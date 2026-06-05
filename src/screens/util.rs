use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;

use super::theme;

/// Natural-scroll offset (see AGENTS.md "Scrolling on overflow"). Keep rows
/// `[start, end]` visible inside a `visible`-row window over `total` rows,
/// moving the prior `cur` offset only when the span crosses an edge.
pub(crate) fn natural_scroll(
    cur: usize,
    start: usize,
    end: usize,
    visible: usize,
    total: usize,
) -> usize {
    if visible == 0 {
        return 0;
    }
    let max_off = total.saturating_sub(visible);
    let cur = cur.min(max_off);
    if start < cur {
        start
    } else if end.saturating_add(1) > cur.saturating_add(visible) {
        end.saturating_add(1).saturating_sub(visible).min(max_off)
    } else {
        cur
    }
}

/// A resolved scroll window over a fixed-height list: the natural-scroll top
/// `offset` and the `range` of row indices that fall inside the window.
///
/// `offset` (== `range.start`) is what callers persist in their `Cell<usize>`,
/// and what `Paragraph`-scrolled lists hand to `Paragraph::scroll`. Lists that
/// slice their own rendered rows iterate `range` instead of re-deriving
/// `skip`/`take` bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScrollWindow {
    pub offset: usize,
    pub range: std::ops::Range<usize>,
}

/// Resolve the [`ScrollWindow`] for a `visible`-row window over `total` rows,
/// keeping rows `[start, end]` on screen. `start`/`end` are the first and last
/// row occupied by the highlighted item — equal for single-row items, or the
/// span of a grouped multi-line row. Wraps [`natural_scroll`] for the offset,
/// then clamps the visible range to `total`.
pub(crate) fn scroll_window(
    cur: usize,
    start: usize,
    end: usize,
    visible: usize,
    total: usize,
) -> ScrollWindow {
    let offset = natural_scroll(cur, start, end, visible, total);
    ScrollWindow {
        offset,
        range: offset..offset.saturating_add(visible).min(total),
    }
}

/// Centered `width` by `height` sub-rect of `area`.
pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// Paint a dim backdrop, clear the centered modal rect, and return it.
pub(crate) fn open_modal(frame: &mut Frame, area: Rect, width: u16, height: u16) -> Rect {
    frame.render_widget(theme::backdrop(), area);
    let rect = centered_rect(area, width, height);
    frame.render_widget(Clear, rect);
    rect
}

/// Truncate to at most `width` display chars, suffixing with "…" if cut.
/// Returns `(string, was_truncated)`.
pub(crate) fn truncate_ellipsis(value: &str, width: usize) -> (String, bool) {
    if width == 0 {
        return (String::new(), !value.is_empty());
    }
    let count = value.chars().count();
    if count <= width {
        return (value.to_string(), false);
    }
    let take = width.saturating_sub(1);
    let mut out: String = value.chars().take(take).collect();
    out.push('…');
    (out, true)
}

pub(crate) fn truncate(value: &str, width: usize) -> String {
    truncate_ellipsis(value, width).0
}

pub(crate) fn wrap_chars(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let word_len = word.chars().count();
        let current_len = current.chars().count();

        if current_len > 0 && current_len + 1 + word_len > width {
            lines.push(current);
            current = String::new();
        }

        if word_len <= width {
            if current.is_empty() {
                current.push_str(word);
            } else {
                current.push(' ');
                current.push_str(word);
            }
            continue;
        }

        let mut chunk = String::new();
        for ch in word.chars() {
            if chunk.chars().count() == width {
                lines.push(chunk);
                chunk = String::new();
            }
            chunk.push(ch);
        }
        current = chunk;
        if current.chars().count() == width {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_scroll_moves_up_when_span_crosses_top() {
        assert_eq!(natural_scroll(5, 3, 4, 6, 20), 3);
    }

    #[test]
    fn natural_scroll_moves_down_when_span_crosses_bottom() {
        assert_eq!(natural_scroll(2, 7, 9, 6, 20), 4);
    }

    #[test]
    fn natural_scroll_holds_when_span_is_visible() {
        assert_eq!(natural_scroll(3, 5, 7, 6, 20), 3);
    }

    #[test]
    fn natural_scroll_zero_visible_returns_zero() {
        assert_eq!(natural_scroll(9, 9, 9, 0, 20), 0);
    }

    #[test]
    fn natural_scroll_total_shorter_than_window_returns_zero() {
        assert_eq!(natural_scroll(3, 1, 2, 10, 4), 0);
    }

    #[test]
    fn natural_scroll_pre_clamps_stale_offset() {
        assert_eq!(natural_scroll(99, 4, 5, 6, 10), 4);
    }

    #[test]
    fn scroll_window_tracks_natural_scroll_offset() {
        // Same span/window as `natural_scroll_moves_down_when_span_crosses_bottom`.
        let window = scroll_window(2, 7, 9, 6, 20);
        assert_eq!(window.offset, 4);
        assert_eq!(window.range, 4..10);
    }

    #[test]
    fn scroll_window_range_starts_at_offset_and_clamps_to_total() {
        // Window taller than the available rows: the range stops at `total`.
        let window = scroll_window(0, 0, 0, 6, 4);
        assert_eq!(window.offset, 0);
        assert_eq!(window.range, 0..4);
    }

    #[test]
    fn centered_rect_centers_with_odd_remainder_at_bottom_right() {
        let area = Rect::new(10, 20, 101, 51);
        assert_eq!(centered_rect(area, 40, 10), Rect::new(40, 40, 40, 10));
    }
}
