/// Slugify a free-text title into a branch-safe identifier.
///
/// Rules:
/// - ASCII alphanumerics are kept (lowercased).
/// - Any other run becomes a single dash.
/// - Leading/trailing dashes trimmed.
/// - Result truncated to 64 characters.
/// - Empty input returns empty string (callers must validate).
pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.truncate(64);
    // Truncation may have left a trailing dash if cut mid-token; trim again.
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_typical_title() {
        assert_eq!(slugify("Add foo to bar"), "add-foo-to-bar");
    }

    #[test]
    fn collapses_runs_of_non_alphanumeric() {
        assert_eq!(slugify("Fix: !@# things & stuff!"), "fix-things-stuff");
    }

    #[test]
    fn trims_leading_and_trailing_punctuation() {
        assert_eq!(slugify("...hello..."), "hello");
    }

    #[test]
    fn truncates_long_titles_to_64_chars() {
        let title = "a".repeat(200);
        let out = slugify(&title);
        assert!(out.len() <= 64);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("!!!"), "");
    }
}
