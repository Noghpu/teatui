/// Open a URL in the system default browser.
///
/// Wraps `opener::open` and maps any underlying error to a short user-facing
/// string. On Windows this uses `ShellExecute`; the `opener` crate handles
/// quoting automatically.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    opener::open(url).map_err(|e| format!("failed to open browser: {e}"))
}

/// Copy text to the system clipboard.
///
/// Wraps `arboard::Clipboard` and maps errors to short user-facing strings.
/// On Linux this requires an X11 or Wayland display; on Windows and macOS it
/// works unconditionally.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard write failed: {e}"))
}
