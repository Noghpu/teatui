//! Minimal blocking HTTP/1.1 POST client with mid-flight cancellation.
//!
//! This exists for exactly one job: the local LLM generation request, which
//! can run for tens of seconds and which the user must be able to abort. ureq
//! (used everywhere else) gives no abort handle, so for the cancellable path we
//! drop to a tiny std-only client: a [`CancelHandle`] holds a clone of the
//! request socket and `shutdown`s it from another thread, unblocking the
//! worker's read/write so the request fails fast and the server frees its slot.
//!
//! Deliberately narrow: `http://` only (local Ollama / llama.cpp), one POST,
//! `Connection: close` so the body is delimited by EOF. Anything fancier
//! (HTTPS, keep-alive, redirects) stays on ureq.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Cancellation handle for an in-flight [`post_json`]. Clones share one inner
/// state, so a clone handed to the worker and a clone held by the UI both refer
/// to the same request. `cancel` is safe to call from any thread, at any time —
/// before the socket exists (the flag short-circuits the connect) or during the
/// transfer (the socket shutdown unblocks the blocked read/write).
#[derive(Clone, Default)]
pub struct CancelHandle(Arc<Inner>);

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    socket: Mutex<Option<TcpStream>>,
}

impl CancelHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the request cancelled and tear down its socket if one is live.
    /// Idempotent; a no-op once the request has finished and cleared its socket.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        if let Some(socket) = self.0.socket.lock().unwrap().take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    fn register(&self, socket: TcpStream) {
        *self.0.socket.lock().unwrap() = Some(socket);
    }

    fn clear(&self) {
        let _ = self.0.socket.lock().unwrap().take();
    }
}

#[derive(Debug)]
pub enum HttpError {
    /// The request was cancelled via its [`CancelHandle`].
    Cancelled,
    /// A connect/read/write exceeded the timeout.
    Timeout,
    Io(std::io::Error),
    /// The response was not recognizable HTTP/1.1.
    Protocol(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Cancelled => write!(f, "request cancelled"),
            HttpError::Timeout => write!(f, "request timed out"),
            HttpError::Io(e) => write!(f, "io error: {e}"),
            HttpError::Protocol(m) => write!(f, "malformed http response: {m}"),
        }
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// POST a JSON body and read the full response. The `timeout` bounds connect,
/// read, and write individually. `cancel` can abort the request at any point.
///
/// Returns the response for *any* HTTP status (including 4xx/5xx) — the caller
/// inspects `status` and reads the error body itself. Only transport problems
/// (cancel, timeout, io, malformed response) come back as `Err`.
pub fn post_json(
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<HttpResponse, HttpError> {
    let target = Target::parse(url)?;
    if cancel.is_cancelled() {
        return Err(HttpError::Cancelled);
    }

    let addr = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(HttpError::Io)?
        .next()
        .ok_or_else(|| HttpError::Protocol(format!("could not resolve {}", target.host)))?;

    let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| classify(cancel, e))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(HttpError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(HttpError::Io)?;

    // Hand a clone of the socket to the cancel handle so another thread can
    // shut it down. Shutdown on either clone tears down the shared fd.
    cancel.register(stream.try_clone().map_err(HttpError::Io)?);
    // Closes the race where cancel() fired between the connect and the register
    // (it found no socket to shut down): bail now rather than start a request
    // that nothing will interrupt.
    if cancel.is_cancelled() {
        cancel.clear();
        return Err(HttpError::Cancelled);
    }

    let result = exchange(&stream, &target, headers, body, cancel);
    // Drop our socket reference so a late cancel() can't shutdown a reused fd.
    cancel.clear();
    result
}

/// Map an io error to the right `HttpError`: a fired cancel always wins (a
/// shutdown surfaces as a generic io error), then timeouts, then everything else.
fn classify(cancel: &CancelHandle, error: std::io::Error) -> HttpError {
    if cancel.is_cancelled() {
        HttpError::Cancelled
    } else if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
        HttpError::Timeout
    } else {
        HttpError::Io(error)
    }
}

fn exchange(
    stream: &TcpStream,
    target: &Target,
    headers: &[(&str, &str)],
    body: &str,
    cancel: &CancelHandle,
) -> Result<HttpResponse, HttpError> {
    // `&TcpStream` implements Read/Write; bind mutably to call the methods.
    let mut stream = stream;

    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {len}\r\n",
        path = target.path,
        host = target.host_header(),
        len = body.len(),
    );
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    request.push_str(body);

    stream
        .write_all(request.as_bytes())
        .map_err(|e| classify(cancel, e))?;
    stream.flush().map_err(|e| classify(cancel, e))?;

    // `Connection: close` means the server closes the socket when done, so
    // read-to-EOF yields the whole response without parsing Content-Length.
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| classify(cancel, e))?;

    // A cancel() shutdown surfaces here as a clean EOF (the read returns the
    // bytes so far, not an error), so check the flag before trusting `raw`.
    if cancel.is_cancelled() {
        return Err(HttpError::Cancelled);
    }

    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<HttpResponse, HttpError> {
    let split = find(raw, b"\r\n\r\n")
        .ok_or_else(|| HttpError::Protocol("no header/body separator".into()))?;
    let header_text = String::from_utf8_lossy(&raw[..split]);
    let body_bytes = &raw[split + 4..];

    let status_line = header_text
        .lines()
        .next()
        .ok_or_else(|| HttpError::Protocol("empty response".into()))?;
    // "HTTP/1.1 200 OK" -> 200
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| HttpError::Protocol(format!("bad status line: {status_line}")))?;

    let body_bytes = dechunk(&header_text, body_bytes)?;
    Ok(HttpResponse {
        status,
        body: String::from_utf8_lossy(&body_bytes).into_owned(),
    })
}

/// Non-streamed responses from Ollama / llama.cpp carry `Content-Length`, so the
/// body is already plain. But a server is free to chunk even under
/// `Connection: close`, so decode it when the header says so rather than feed a
/// chunk-framed body to the JSON parser.
fn dechunk(header_text: &str, body: &[u8]) -> Result<Vec<u8>, HttpError> {
    let chunked = header_text.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        line.starts_with("transfer-encoding:") && line.contains("chunked")
    });
    if !chunked {
        return Ok(body.to_vec());
    }

    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let nl =
            find(rest, b"\r\n").ok_or_else(|| HttpError::Protocol("unterminated chunk".into()))?;
        // The chunk-size line may carry `;ext` extensions; the size is the prefix.
        let size_field = String::from_utf8_lossy(&rest[..nl]);
        let size_hex = size_field.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| HttpError::Protocol(format!("bad chunk size: {size_hex}")))?;
        rest = &rest[nl + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size {
            return Err(HttpError::Protocol("truncated chunk".into()));
        }
        out.extend_from_slice(&rest[..size]);
        // Each chunk's data is followed by a CRLF.
        rest = rest.get(size + 2..).unwrap_or(&[]);
    }
    Ok(out)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

struct Target {
    host: String,
    port: u16,
    path: String,
}

impl Target {
    fn parse(url: &str) -> Result<Self, HttpError> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| HttpError::Protocol(format!("not an http:// url: {url}")))?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host.to_string(),
                port.parse::<u16>()
                    .map_err(|_| HttpError::Protocol(format!("bad port in {url}")))?,
            ),
            None => (authority.to_string(), 80),
        };
        Ok(Self {
            host,
            port,
            path: if path.is_empty() {
                "/".into()
            } else {
                path.to_string()
            },
        })
    }

    /// `Host` omits the port when it's the default, per HTTP convention.
    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Spawn a listener that handles exactly one connection with `handler`,
    /// returning the bound `http://` URL.
    fn serve_once<F>(handler: F) -> String
    where
        F: FnOnce(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/endpoint", listener.local_addr().unwrap());
        thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handler(stream);
            }
        });
        url
    }

    /// Read the client's request headers (up to the blank line) off the socket
    /// so the connection is fully established before the handler responds.
    fn drain_request(stream: &mut TcpStream) {
        let mut buf = [0u8; 1024];
        // A single read is enough to get past the request line for these tests.
        let _ = stream.read(&mut buf);
    }

    #[test]
    fn reads_canned_ok_response() {
        let url = serve_once(|mut stream| {
            drain_request(&mut stream);
            let body = r#"{"ok":true}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let cancel = CancelHandle::new();
        let resp = post_json(&url, &[], "{}", Duration::from_secs(5), &cancel).expect("response");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, r#"{"ok":true}"#);
    }

    #[test]
    fn surfaces_status_error_body() {
        let url = serve_once(|mut stream| {
            drain_request(&mut stream);
            let body = r#"{"error":{"message":"too long"}}"#;
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let cancel = CancelHandle::new();
        let resp = post_json(&url, &[], "{}", Duration::from_secs(5), &cancel).expect("response");
        assert_eq!(resp.status, 400);
        assert!(resp.body.contains("too long"));
    }

    #[test]
    fn decodes_chunked_body() {
        let url = serve_once(|mut stream| {
            drain_request(&mut stream);
            // "Wiki" + "pedia" split across two chunks.
            let response = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n\
                4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
            stream.write_all(response.as_bytes()).unwrap();
        });

        let cancel = CancelHandle::new();
        let resp = post_json(&url, &[], "{}", Duration::from_secs(5), &cancel).expect("response");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "Wikipedia");
    }

    #[test]
    fn cancel_unblocks_a_stalled_request() {
        // Server accepts but never replies, holding the socket open.
        let url = serve_once(|mut stream| {
            drain_request(&mut stream);
            thread::sleep(Duration::from_secs(30));
        });

        let cancel = CancelHandle::new();
        let canceller = cancel.clone();
        let waiter = thread::spawn(move || {
            // Generous timeout so it's the cancel, not the timeout, that wins.
            post_json(&url, &[], "{}", Duration::from_secs(30), &cancel)
        });
        // Give the request time to connect and register its socket, then abort.
        thread::sleep(Duration::from_millis(200));
        canceller.cancel();

        let result = waiter.join().expect("thread");
        assert!(
            matches!(result, Err(HttpError::Cancelled)),
            "got {result:?}"
        );
    }

    #[test]
    fn cancel_before_request_short_circuits() {
        let cancel = CancelHandle::new();
        cancel.cancel();
        let result = post_json(
            "http://127.0.0.1:9/endpoint",
            &[],
            "{}",
            Duration::from_secs(5),
            &cancel,
        );
        assert!(
            matches!(result, Err(HttpError::Cancelled)),
            "got {result:?}"
        );
    }

    #[test]
    fn parses_url_with_default_port() {
        let target = Target::parse("http://example.test/v1/chat").expect("parse");
        assert_eq!(target.host, "example.test");
        assert_eq!(target.port, 80);
        assert_eq!(target.path, "/v1/chat");
        assert_eq!(target.host_header(), "example.test");
    }

    #[test]
    fn parses_url_with_explicit_port() {
        let target = Target::parse("http://localhost:11434/api/generate").expect("parse");
        assert_eq!(target.host, "localhost");
        assert_eq!(target.port, 11434);
        assert_eq!(target.path, "/api/generate");
        assert_eq!(target.host_header(), "localhost:11434");
    }
}
