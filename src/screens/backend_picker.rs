//! Modal overlay for switching the active LLM backend. It floats over
//! whichever screen is focused (Landing or Generate), so its state lives
//! on `App` rather than in a single screen. Opening it probes every
//! configured backend's health; each row resolves to ✓ reachable /
//! ✗ unreachable / ◌ pending, colored normal / warning / faded to match.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::config::LlmBackend;
use crate::domain::{LlmHealth, StatusStore};
use crate::runtime::Cached;

use super::theme;

#[derive(Debug, Default)]
pub struct BackendPicker {
    pub highlighted: usize,
    /// Top index of the scrolled list window. A `Cell` so render can keep it in
    /// sync with `highlighted` without a mutable borrow; edge-clamped there.
    pub scroll: Cell<usize>,
}

/// What the App should do after handing a key to the open picker.
pub enum PickerOutcome {
    /// Key was irrelevant; no redraw needed.
    None,
    /// Selection moved; redraw.
    Dirty,
    /// Dismiss without switching.
    Close,
    /// Commit the backend at this index as the new active backend.
    Select(usize),
}

impl BackendPicker {
    /// Open with the cursor on the currently-active backend so Enter is a
    /// no-op confirm and arrows move away from it.
    pub fn new(active: &str, backends: &[LlmBackend]) -> Self {
        let highlighted = backends.iter().position(|b| b.name == active).unwrap_or(0);
        Self {
            highlighted,
            scroll: Cell::new(0),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, count: usize) -> PickerOutcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            // `b` toggles the switcher closed, mirroring the open shortcut.
            (KeyCode::Esc, _) | (KeyCode::Char('b'), false) | (KeyCode::Char('c'), true) => {
                PickerOutcome::Close
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), false) => {
                if self.highlighted > 0 {
                    self.highlighted -= 1;
                    PickerOutcome::Dirty
                } else {
                    PickerOutcome::None
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), false) => {
                if self.highlighted + 1 < count {
                    self.highlighted += 1;
                    PickerOutcome::Dirty
                } else {
                    PickerOutcome::None
                }
            }
            (KeyCode::Enter, _) if count > 0 => PickerOutcome::Select(self.highlighted),
            _ => PickerOutcome::None,
        }
    }
}

pub fn render(
    picker: &BackendPicker,
    backends: &[LlmBackend],
    active: &str,
    status: &StatusStore,
    frame: &mut Frame,
    area: Rect,
) {
    frame.render_widget(theme::backdrop(), area);

    let width = area.width.saturating_sub(8).clamp(40, 72);
    // backends + border (2) + blank + hint (2), clamped to the screen.
    let desired = backends.len() as u16 + 5;
    let height = desired.clamp(6, area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);

    let block = theme::modal_block("Switch LLM backend");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    // Reserve the last two inner rows for a blank + hint line. Scroll naturally:
    // hold the window steady and only move it when the highlight crosses an edge.
    let list_rows = (inner.height as usize).saturating_sub(2).max(1);
    let max_offset = backends.len().saturating_sub(list_rows);
    let cur = picker.scroll.get().min(max_offset);
    let offset = if picker.highlighted < cur {
        picker.highlighted
    } else if picker.highlighted >= cur + list_rows {
        picker.highlighted - list_rows + 1
    } else {
        cur
    };
    picker.scroll.set(offset);

    let mut lines: Vec<Line> = Vec::new();
    if backends.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no backends configured",
            theme::muted(),
        )));
    } else {
        for (idx, backend) in backends.iter().enumerate().skip(offset).take(list_rows) {
            lines.push(backend_row(
                backend,
                idx == picker.highlighted,
                backend.name == active,
                status.backend_health(&backend.name),
                inner.width as usize,
            ));
        }
    }

    lines.push(Line::from(""));
    lines.push(theme::help_line(
        &[
            theme::HelpHint::primary("Enter", "switch"),
            theme::HelpHint::new("↑↓", "move"),
            theme::HelpHint::new("Esc", "cancel"),
        ],
        inner.width,
    ));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn backend_row(
    backend: &LlmBackend,
    highlighted: bool,
    is_active: bool,
    health: Option<&Cached<LlmHealth>>,
    width: usize,
) -> Line<'static> {
    let (glyph, style) = health_indicator(health);
    let style = if highlighted {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    };

    let marker_style = if highlighted {
        theme::selected(true)
    } else {
        theme::muted()
    };
    let dot = if is_active { "● " } else { "  " };
    let dot_style = if is_active {
        theme::accent()
    } else {
        theme::muted()
    };

    // Layout: "▶ " + "● " + body (padded) + " ✓". Fixed parts use 6 cols.
    let body_w = width.saturating_sub(6);
    let body = truncate(
        &format!(
            "{}   {}   {}",
            backend.name, backend.model, backend.base_url
        ),
        body_w,
    );
    let pad = " ".repeat(body_w.saturating_sub(body.chars().count()));

    Line::from(vec![
        Span::styled(
            theme::selection_marker(highlighted).to_string(),
            marker_style,
        ),
        Span::styled(dot.to_string(), dot_style),
        Span::styled(format!("{body}{pad}"), style),
        Span::styled(format!(" {glyph}"), style),
    ])
}

/// Map cached health to (glyph, style): reachable → ✓ normal text,
/// unreachable → ✗ warning color, pending/never-probed → ◌ faded.
fn health_indicator(health: Option<&Cached<LlmHealth>>) -> (&'static str, Style) {
    match health.and_then(Cached::value) {
        Some(LlmHealth::Available { .. }) => ("✓", theme::text()),
        Some(LlmHealth::Unreachable { .. }) => ("✗", theme::warning()),
        None => ("◌", theme::muted()),
    }
}

fn truncate(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if value.chars().count() <= width {
        return value.to_string();
    }
    let mut out: String = value.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backends() -> Vec<LlmBackend> {
        vec![
            LlmBackend {
                name: "default".into(),
                ..Default::default()
            },
            LlmBackend {
                name: "fast".into(),
                ..Default::default()
            },
            LlmBackend {
                name: "cloud".into(),
                ..Default::default()
            },
        ]
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn opens_on_active_backend() {
        let picker = BackendPicker::new("fast", &backends());
        assert_eq!(picker.highlighted, 1);
    }

    #[test]
    fn unknown_active_defaults_to_first() {
        let picker = BackendPicker::new("missing", &backends());
        assert_eq!(picker.highlighted, 0);
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut picker = BackendPicker::new("default", &backends());
        assert!(matches!(
            picker.on_key(key(KeyCode::Up), 3),
            PickerOutcome::None
        ));
        assert_eq!(picker.highlighted, 0);
        let _ = picker.on_key(key(KeyCode::Down), 3);
        let _ = picker.on_key(key(KeyCode::Down), 3);
        assert!(matches!(
            picker.on_key(key(KeyCode::Down), 3),
            PickerOutcome::None
        ));
        assert_eq!(picker.highlighted, 2);
    }

    #[test]
    fn enter_selects_highlighted_index() {
        let mut picker = BackendPicker::new("default", &backends());
        let _ = picker.on_key(key(KeyCode::Down), 3);
        match picker.on_key(key(KeyCode::Enter), 3) {
            PickerOutcome::Select(idx) => assert_eq!(idx, 1),
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn esc_and_b_close() {
        let mut picker = BackendPicker::new("default", &backends());
        assert!(matches!(
            picker.on_key(key(KeyCode::Esc), 3),
            PickerOutcome::Close
        ));
        assert!(matches!(
            picker.on_key(key(KeyCode::Char('b')), 3),
            PickerOutcome::Close
        ));
    }
}
