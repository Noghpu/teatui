use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::screens::generate::form::TextFieldState;

use super::{theme, util};

pub(crate) fn form_line(frame: &mut Frame, inner: Rect, cy: u16, scroll: u16, line: Line<'static>) {
    let sy = inner.y as i32 + cy as i32 - scroll as i32;
    if sy >= inner.y as i32 && (sy as u16) < inner.y + inner.height {
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: inner.x,
                y: sy as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
}

pub(crate) fn form_block(
    frame: &mut Frame,
    inner: Rect,
    cy: u16,
    scroll: u16,
    lines: Vec<Line<'static>>,
) {
    let fh = lines.len() as u16;
    let sy = inner.y as i32 + cy as i32 - scroll as i32;
    let vp_bot = inner.y + inner.height;
    if sy >= vp_bot as i32 || sy + fh as i32 <= inner.y as i32 {
        return;
    }
    let skip = (inner.y as i32 - sy).max(0) as u16;
    let vis_y = sy.max(inner.y as i32) as u16;
    let vis_h = fh.saturating_sub(skip).min(vp_bot - vis_y);
    if vis_h > 0 {
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((skip, 0)),
            Rect {
                x: inner.x,
                y: vis_y,
                width: inner.width,
                height: vis_h,
            },
        );
    }
}

/// Render one editable text field (label row + value box) using a
/// viewport-clipped positional model.
///
/// Error lines are NOT included; callers append them when needed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_text_field(
    frame: &mut Frame,
    inner: Rect,
    cy: &mut u16,
    scroll: u16,
    t: &TextFieldState,
    label: &str,
    marker: &str,
    label_style: Style,
    editing: bool,
    value_w: usize,
) {
    form_line(
        frame,
        inner,
        *cy,
        scroll,
        Line::from(Span::styled(format!("{marker}{label}:"), label_style)),
    );
    *cy += 1;

    let indent: u16 = 2;
    let value_h: u16 = if t.multiline {
        multiline_value_height(t, value_w, editing)
    } else {
        1
    };

    let sy = inner.y as i32 + *cy as i32 - scroll as i32;
    let vp_top = inner.y as i32;
    let vp_bot = inner.y.saturating_add(inner.height);
    if sy < vp_bot as i32 && sy + value_h as i32 > vp_top {
        let skip = (vp_top - sy).max(0) as u16;
        let vis_y = sy.max(vp_top) as u16;
        let vis_h = value_h.saturating_sub(skip).min(vp_bot - vis_y);
        if vis_h > 0 {
            if editing {
                let rect = Rect {
                    x: inner.x + indent,
                    y: vis_y,
                    width: inner.width.saturating_sub(indent),
                    height: vis_h,
                };
                frame.render_widget(&t.editor, rect);
            } else {
                let rect = Rect {
                    x: inner.x,
                    y: vis_y,
                    width: inner.width,
                    height: vis_h,
                };
                let lines = if t.multiline {
                    let mut v: Vec<Line> = t
                        .value
                        .lines()
                        .flat_map(|l| {
                            util::wrap_chars(l, value_w)
                                .into_iter()
                                .map(|s| Line::from(Span::styled(format!("  {s}"), theme::text())))
                        })
                        .take(value_h as usize)
                        .collect();
                    let total_lines: usize = t
                        .value
                        .lines()
                        .map(|l| util::wrap_chars(l, value_w).len())
                        .sum();
                    if total_lines > value_h as usize
                        && let Some(last) = v.last_mut()
                    {
                        *last = Line::from(Span::styled("  …", theme::muted()));
                    }
                    if v.is_empty() {
                        v.push(empty_value_line());
                    }
                    while v.len() < value_h as usize {
                        v.push(Line::from(""));
                    }
                    v
                } else if t.value.is_empty() {
                    vec![empty_value_line()]
                } else {
                    let (display, _) = util::truncate_ellipsis(&t.value, value_w);
                    vec![Line::from(Span::styled(
                        format!("  {display}"),
                        theme::text(),
                    ))]
                };
                frame.render_widget(Paragraph::new(lines).scroll((skip, 0)), rect);
            }
        }
    }
    *cy += value_h;
}

/// Display height of a multiline text field's value box: tall enough to show all
/// the wrapped content, but never shorter than the familiar minimum size.
pub(crate) fn multiline_value_height(t: &TextFieldState, value_w: usize, editing: bool) -> u16 {
    const MULTILINE_MIN_HEIGHT: u16 = 6;
    let content = if editing { &t.buffer } else { &t.value };
    let lines: usize = if content.is_empty() {
        1
    } else {
        content
            .lines()
            .map(|l| util::wrap_chars(l, value_w).len())
            .sum()
    };
    (lines as u16).max(MULTILINE_MIN_HEIGHT)
}

pub(crate) fn placeholder_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), theme::muted()))
}

/// Empty-textarea placeholder, indented to align with normal value lines.
pub(crate) fn empty_value_line() -> Line<'static> {
    Line::from(Span::styled("  (empty)", theme::muted()))
}

pub(crate) fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {text}"), theme::muted()))
}

pub(crate) fn kv_line(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key}: "), theme::muted()),
        Span::styled(value, theme::text()),
    ])
}

pub(crate) fn kv_line_fit(key: &str, value: &str, width: usize) -> Line<'static> {
    let prefix = format!("  {key}: ");
    let prefix_width = prefix.chars().count();
    if width <= prefix_width {
        let (display, _) = util::truncate_ellipsis(&prefix, width);
        return Line::from(Span::styled(display, theme::muted()));
    }

    let (display, _) = util::truncate_ellipsis(value, width - prefix_width);
    Line::from(vec![
        Span::styled(prefix, theme::muted()),
        Span::styled(display, theme::text()),
    ])
}

pub(crate) fn wrapped_styled_lines(
    prefix: &str,
    text: &str,
    width: usize,
    style: Style,
) -> Vec<Line<'static>> {
    wrap_prefixed(prefix, text, width)
        .into_iter()
        .map(|line| Line::from(Span::styled(line, style)))
        .collect()
}

pub(crate) fn field_header_line(marker: &str, label: &str, style: Style) -> Line<'static> {
    Line::from(Span::styled(format!("{marker}{label}:"), style))
}

pub(crate) fn status_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {text}"),
        theme::accent().add_modifier(Modifier::BOLD),
    ))
}

pub(crate) fn section_header(title: &str) -> Vec<Line<'static>> {
    vec![Line::from(""), section_heading_line(title)]
}

pub(crate) fn section_heading_line(title: &str) -> Line<'static> {
    theme::header(format!("{title}:"))
}

pub(crate) fn separator_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}  ", "─".repeat(width.saturating_sub(4))),
        Style::default().fg(theme::BORDER),
    ))
}

fn wrap_prefixed(prefix: &str, text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let prefix_width = prefix.chars().count();
    if width <= prefix_width {
        let (display, _) = util::truncate_ellipsis(prefix, width);
        let mut lines = vec![display];
        for physical in text.lines() {
            lines.extend(util::wrap_chars(physical, width));
        }
        if text.is_empty() {
            lines.push(String::new());
        }
        return lines;
    }

    let continuation = " ".repeat(prefix_width);
    let text_width = width - prefix_width;
    let mut lines = Vec::new();
    let mut first = true;
    for physical in text.lines() {
        for wrapped in util::wrap_chars(physical, text_width) {
            if first {
                lines.push(format!("{prefix}{wrapped}"));
                first = false;
            } else {
                lines.push(format!("{continuation}{wrapped}"));
            }
        }
    }

    if lines.is_empty() {
        lines.push(prefix.to_string());
    }
    lines
}
