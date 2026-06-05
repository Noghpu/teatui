use std::io;
use std::process::{Command, Output, Stdio};

/// Run `binary` with `args`, with stdin nulled and stdout/stderr captured,
/// returning the raw `Output`.
///
/// This is the low-level layer: it preserves the `io::Result` so callers can
/// distinguish a missing binary (`io::ErrorKind::NotFound`) from a command that
/// ran and failed, and can inspect exit status, stdout, and stderr separately.
/// [`capture`] builds the success-or-error-string convenience layer on top.
pub(crate) fn output<S: AsRef<str>>(binary: &str, args: &[S]) -> io::Result<Output> {
    Command::new(binary)
        .args(args.iter().map(AsRef::as_ref))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}

/// Run `binary` with `args`, with stdin nulled and stdout/stderr captured.
///
/// On success, returns stdout. On failure, returns
/// `"{binary} {args:?}: {body}"`, where body is trimmed stderr or trimmed
/// stdout when stderr is empty.
pub(crate) fn capture<S: AsRef<str>>(binary: &str, args: &[S]) -> Result<String, String> {
    let shown: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let out = output(binary, &shown).map_err(|e| format!("{binary} {shown:?}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let body = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("{binary} {shown:?}: {body}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn jj_args<S: AsRef<str>>(args: &[S]) -> Vec<&str> {
    let mut all: Vec<&str> = Vec::with_capacity(args.len() + 1);
    all.push("--no-pager");
    all.extend(args.iter().map(AsRef::as_ref));
    all
}

/// Raw jj invocation: always prepend `--no-pager`.
pub(crate) fn jj_output<S: AsRef<str>>(binary: &str, args: &[S]) -> io::Result<Output> {
    let all = jj_args(args);
    output(binary, &all)
}

/// Standard jj invocation: always prepend `--no-pager`.
pub(crate) fn jj<S: AsRef<str>>(binary: &str, args: &[S]) -> Result<String, String> {
    let all = jj_args(args);
    capture(binary, &all)
}

/// tea invocation; no pager flag is needed.
pub(crate) fn tea<S: AsRef<str>>(binary: &str, args: &[S]) -> Result<String, String> {
    capture(binary, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MISSING_BINARY: &str = "__teatui_missing_process_binary__";

    #[test]
    fn output_surfaces_not_found_for_missing_binary() {
        // Probes rely on this to tell a missing tool from a failed command.
        let err = output(MISSING_BINARY, &["whatever"]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn jj_output_surfaces_not_found_for_missing_binary() {
        let err = jj_output(MISSING_BINARY, &["status"]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn capture_error_includes_binary_and_args() {
        let err = capture(MISSING_BINARY, &["arg-one", "arg-two"]).unwrap_err();
        assert!(err.starts_with("__teatui_missing_process_binary__ [\"arg-one\", \"arg-two\"]: "));
    }

    #[test]
    fn jj_error_message_shows_no_pager_prefix() {
        let err = jj(MISSING_BINARY, &["status"]).unwrap_err();
        assert!(
            err.starts_with("__teatui_missing_process_binary__ [\"--no-pager\", \"status\"]: ")
        );
    }

    #[test]
    fn tea_accepts_string_args_without_no_pager() {
        let args = vec!["pr".to_string(), "list".to_string()];
        let err = tea(MISSING_BINARY, &args).unwrap_err();
        assert!(err.starts_with("__teatui_missing_process_binary__ [\"pr\", \"list\"]: "));
    }
}
