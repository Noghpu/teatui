//! Helpers for deterministic auto-bookmark naming used when the PR base or tip
//! lacks an existing bookmark.
//!
//! # Naming rules
//!
//! - **Tip bookmark**: `pr/<slug-of-title>` when a title is available and
//!   non-empty; `pr/<change_id>` as fallback.
//! - **Base bookmark**: `pr-base/<tip-bookmark-without-leading-prefix>`.
//!   Stable and scoped to the PR so it is easy to identify and prune later.
//!
//! # Change-id heuristic
//!
//! A string is treated as a jj change_id when it matches `^[a-z]{8,}$` — all
//! lowercase ASCII letters, at least 8 chars, no separators (`@`, `/`, `:`,
//! etc.).  Remote refs always contain `@` or `/`; branch names that reach this
//! code path never consist of pure lowercase letters.  The heuristic is
//! intentionally conservative: exotic remote refs that happen to look like
//! change_ids are classified as remote refs because they will contain at least
//! one separator character.

/// Converts an arbitrary string into a URL-safe slug for use in bookmark names.
///
/// Rules:
/// - Lower-cased.
/// - Non-alphanumeric ASCII characters are replaced with `-`.
/// - Non-ASCII characters (unicode) are dropped.
/// - Consecutive `-` are collapsed to a single `-`.
/// - Leading and trailing `-` are trimmed.
/// - Truncated to at most 32 characters at a `-` boundary when possible.
pub fn slugify(input: &str) -> String {
    // Replace non-ascii characters with nothing, lowercase everything, and
    // turn non-alphanumeric runs into a single dash.
    let mut slug = String::with_capacity(input.len());
    let mut last_was_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if ch.is_ascii() {
            // ASCII non-alphanumeric → separator
            if !last_was_dash && !slug.is_empty() {
                slug.push('-');
                last_was_dash = true;
            }
        }
        // Non-ASCII is dropped.
    }
    // Trim trailing dash.
    let slug = slug.trim_end_matches('-').to_string();

    if slug.len() <= 32 {
        return slug;
    }

    // Truncate to 32 chars at a dash boundary.
    let truncated = &slug[..32];
    if let Some(pos) = truncated.rfind('-')
        && pos > 0
    {
        return truncated[..pos].to_string();
    }
    truncated.to_string()
}

/// Returns the bookmark name to use for the PR tip change.
///
/// - If `title` is non-empty after trimming, produces `pr/<slug(title)>`.
/// - Otherwise falls back to `pr/<change_id>`.
pub fn tip_bookmark(title: &str, change_id: &str) -> String {
    let title_trimmed = title.trim();
    if title_trimmed.is_empty() {
        format!("pr/{}", change_id)
    } else {
        let slug = slugify(title_trimmed);
        if slug.is_empty() {
            format!("pr/{}", change_id)
        } else {
            format!("pr/{}", slug)
        }
    }
}

/// Returns the bookmark name to use for the PR base change.
///
/// Strips the leading `pr/` prefix (if any) from `tip_bookmark` and produces
/// `pr-base/<rest>`.  If `tip_bookmark` does not start with `pr/`, the full
/// tip bookmark name is used as the suffix.
pub fn base_bookmark(tip_bm: &str) -> String {
    let suffix = tip_bm.strip_prefix("pr/").unwrap_or(tip_bm);
    format!("pr-base/{}", suffix)
}

/// Returns `true` when `s` looks like a jj change_id rather than a remote ref.
///
/// A change_id is `^[a-z]{8,}$`: all lowercase ASCII letters, at least 8
/// characters, no separator characters (`@`, `/`, `:`, space, `-`, digits,
/// etc.).
pub fn is_change_id_like(s: &str) -> bool {
    let s = s.trim();
    s.len() >= 8 && s.chars().all(|ch| ch.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- slugify ---

    #[test]
    fn slugify_simple_ascii_title() {
        assert_eq!(slugify("Add feature X"), "add-feature-x");
    }

    #[test]
    fn slugify_leading_trailing_separators_trimmed() {
        assert_eq!(slugify("  --hello--  "), "hello");
    }

    #[test]
    fn slugify_collapses_repeated_dashes() {
        assert_eq!(slugify("foo---bar"), "foo-bar");
    }

    #[test]
    fn slugify_empty_input() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_unicode_dropped() {
        // Unicode chars are dropped (not treated as separators), ASCII letters kept.
        // "résumé: add" → 'r' + é(dropped) + 'sum' + é(dropped) + ': '(sep) + 'add' → "rsum-add"
        assert_eq!(slugify("résumé: add"), "rsum-add");
    }

    #[test]
    fn slugify_truncates_to_32_at_dash_boundary() {
        let long = "abcdefghijklmnopqrstuvwxyz-more-words-here";
        let result = slugify(long);
        assert!(result.len() <= 32);
        // Should cut at dash boundary.
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn slugify_exactly_32_chars_no_truncation() {
        let exact = "a".repeat(32);
        assert_eq!(slugify(&exact), exact);
    }

    #[test]
    fn slugify_33_chars_no_dash_truncates_hard() {
        let no_dash = "a".repeat(33);
        let result = slugify(&no_dash);
        assert_eq!(result.len(), 32);
    }

    // --- tip_bookmark ---

    #[test]
    fn tip_bookmark_uses_title_slug() {
        assert_eq!(
            tip_bookmark("Add login page", "abcdefgh"),
            "pr/add-login-page"
        );
    }

    #[test]
    fn tip_bookmark_falls_back_to_change_id_when_title_empty() {
        assert_eq!(tip_bookmark("", "abcdefgh"), "pr/abcdefgh");
    }

    #[test]
    fn tip_bookmark_falls_back_to_change_id_when_title_whitespace() {
        assert_eq!(tip_bookmark("   ", "abcdefgh"), "pr/abcdefgh");
    }

    #[test]
    fn tip_bookmark_falls_back_to_change_id_when_slug_empty_after_unicode_drop() {
        // If the title consists only of unicode chars the slug is empty.
        assert_eq!(tip_bookmark("こんにちは", "abcdefgh"), "pr/abcdefgh");
    }

    #[test]
    fn tip_bookmark_very_long_title() {
        let long_title = "This is a very long PR title that exceeds thirty-two characters total";
        let result = tip_bookmark(long_title, "abcdefgh");
        assert!(result.starts_with("pr/"));
        let slug_part = &result["pr/".len()..];
        assert!(slug_part.len() <= 32, "slug too long: {}", slug_part.len());
    }

    // --- base_bookmark ---

    #[test]
    fn base_bookmark_strips_pr_prefix() {
        assert_eq!(base_bookmark("pr/add-login-page"), "pr-base/add-login-page");
    }

    #[test]
    fn base_bookmark_no_pr_prefix_uses_full_name() {
        assert_eq!(base_bookmark("feature/thing"), "pr-base/feature/thing");
    }

    #[test]
    fn base_bookmark_stability() {
        // Calling base_bookmark twice with same input produces same output.
        let tip = tip_bookmark("My feature", "abcdefgh");
        let base = base_bookmark(&tip);
        assert_eq!(base, base_bookmark(&tip));
    }

    // --- is_change_id_like ---

    #[test]
    fn is_change_id_like_pure_lowercase_8_chars() {
        assert!(is_change_id_like("abcdefgh"));
    }

    #[test]
    fn is_change_id_like_pure_lowercase_longer() {
        assert!(is_change_id_like("xyzuvwrqpomn"));
    }

    #[test]
    fn is_change_id_like_too_short() {
        assert!(!is_change_id_like("abcdefg")); // 7 chars
    }

    #[test]
    fn is_change_id_like_contains_at_sign() {
        assert!(!is_change_id_like("main@origin"));
    }

    #[test]
    fn is_change_id_like_contains_slash() {
        assert!(!is_change_id_like("feature/foo"));
    }

    #[test]
    fn is_change_id_like_contains_digits() {
        assert!(!is_change_id_like("abc12345"));
    }

    #[test]
    fn is_change_id_like_contains_uppercase() {
        assert!(!is_change_id_like("AbcDefGh"));
    }

    #[test]
    fn is_change_id_like_empty() {
        assert!(!is_change_id_like(""));
    }

    #[test]
    fn is_change_id_like_with_whitespace_trimmed() {
        // Leading/trailing whitespace is trimmed.
        assert!(is_change_id_like("  abcdefgh  "));
    }
}
