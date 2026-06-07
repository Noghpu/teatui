use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Widget};

pub const BASE: Color = Color::Rgb(30, 30, 46);
pub const MANTLE: Color = Color::Rgb(24, 24, 37);
pub const SURFACE0: Color = Color::Rgb(49, 50, 68);
pub const SURFACE1: Color = Color::Rgb(69, 71, 90);
pub const OVERLAY0: Color = Color::Rgb(108, 112, 134);
pub const OVERLAY1: Color = Color::Rgb(127, 132, 156);
pub const TEXT: Color = Color::Rgb(205, 214, 244);
pub const SUBTEXT1: Color = Color::Rgb(186, 194, 222);
pub const BLUE: Color = Color::Rgb(137, 180, 250);
pub const GREEN: Color = Color::Rgb(166, 227, 161);
pub const RED: Color = Color::Rgb(243, 139, 168);
pub const PEACH: Color = Color::Rgb(250, 179, 135);
pub const YELLOW: Color = Color::Rgb(249, 226, 175);
pub const LAVENDER: Color = Color::Rgb(180, 190, 254);

pub const ACCENT: Color = BLUE;
pub const MUTED: Color = OVERLAY0;
pub const GOOD: Color = GREEN;
pub const BAD: Color = RED;
pub const WARN: Color = PEACH;
pub const BORDER: Color = SURFACE0;
pub const FOCUSED_BORDER: Color = BLUE;

pub fn root_block(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title.into()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .padding(Padding::horizontal(1))
}

pub fn pane_block(title: impl Into<String>, focused: bool) -> Block<'static> {
    let border = if focused { FOCUSED_BORDER } else { BORDER };
    let title_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(SUBTEXT1)
    };
    Block::default()
        .title(Span::styled(format!(" {} ", title.into()), title_style))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .padding(Padding::horizontal(1))
}

/// Dim every cell in the given area so a modal floats on top of a darkened
/// backdrop. We don't replace symbols — we just dim foreground and recolor
/// background to MANTLE so unrelated panes read as "behind" the modal.
pub struct Backdrop;

pub fn backdrop() -> Backdrop {
    Backdrop
}

impl Widget for Backdrop {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(MANTLE);
                    cell.modifier.insert(Modifier::DIM);
                }
            }
        }
    }
}

pub fn modal_block(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title.into()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .padding(Padding::horizontal(1))
}

pub fn text() -> Style {
    Style::default().fg(TEXT)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED)
}

pub fn subtle() -> Style {
    Style::default().fg(OVERLAY1)
}

pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}

pub fn selected(focused: bool) -> Style {
    let style = Style::default().fg(ACCENT);
    if focused {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

pub fn success() -> Style {
    Style::default().fg(GOOD)
}

pub fn warning() -> Style {
    Style::default().fg(WARN)
}

pub fn error() -> Style {
    Style::default().fg(BAD)
}

pub fn header(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        muted().add_modifier(Modifier::BOLD),
    ))
}

pub fn hint(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(text.into(), muted()))
}

pub fn kv(key: impl Into<String>, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<12}", key.into()), muted()),
        Span::styled(value.into(), text()),
    ])
}

pub fn badge(label: impl Into<String>, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {} ", label.into()),
            Style::default()
                .fg(BASE)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", value.into()), subtle()),
    ])
}

pub fn selection_marker(selected: bool) -> &'static str {
    if selected { "▶ " } else { "  " }
}

pub fn footer(mode: &str, segments: &[&str]) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!(" {mode} "),
        Style::default()
            .fg(BASE)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD),
    )];
    for segment in segments {
        spans.push(Span::styled(format!(" {segment} "), muted()));
    }
    Line::from(spans)
}

/// A chip in the status bar. Lower `priority` is shed first when the bar
/// would overflow the available width. `mode` chips render inverted; plain
/// chips render muted with a thin separator before them.
pub struct StatusChip {
    pub label: String,
    pub priority: u8,
    pub kind: ChipKind,
}

pub enum ChipKind {
    Mode,
    Plain,
    Styled(Style),
}

impl StatusChip {
    pub fn mode(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            priority: u8::MAX,
            kind: ChipKind::Mode,
        }
    }

    pub fn plain(label: impl Into<String>, priority: u8) -> Self {
        Self {
            label: label.into(),
            priority,
            kind: ChipKind::Plain,
        }
    }

    pub fn styled(label: impl Into<String>, priority: u8, style: Style) -> Self {
        Self {
            label: label.into(),
            priority,
            kind: ChipKind::Styled(style),
        }
    }

    fn width(&self) -> usize {
        // " {label} " for the chip itself.
        self.label.chars().count() + 2
    }
}

/// Render chips to a single status line, dropping the lowest-priority
/// chips first if total width would exceed `area_width`. The Mode chip
/// is always kept (priority u8::MAX) and rendered first.
pub fn status_line(chips: Vec<StatusChip>, area_width: u16) -> Line<'static> {
    let sep_width = 3; // " │ " between chips
    let mut kept = chips;
    loop {
        let total: usize = kept.iter().map(StatusChip::width).sum::<usize>()
            + sep_width * kept.len().saturating_sub(1);
        if total <= area_width as usize {
            break;
        }
        // Drop the lowest-priority chip; ties keep the leftmost (so we
        // shed rightward chips first when priority is equal).
        let Some((idx, _)) = kept
            .iter()
            .enumerate()
            .filter(|(_, c)| !matches!(c.kind, ChipKind::Mode))
            .min_by_key(|(i, c)| (c.priority, std::cmp::Reverse(*i)))
        else {
            break;
        };
        kept.remove(idx);
    }
    let mut spans = Vec::new();
    for (i, chip) in kept.into_iter().enumerate() {
        match chip.kind {
            ChipKind::Mode => {
                spans.push(Span::styled(
                    format!(" {} ", chip.label),
                    Style::default()
                        .fg(BASE)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            ChipKind::Plain | ChipKind::Styled(_) => {
                if i > 0 {
                    spans.push(Span::styled(" │ ", subtle()));
                }
                let style = match chip.kind {
                    ChipKind::Plain => muted(),
                    ChipKind::Styled(style) => style,
                    ChipKind::Mode => unreachable!(),
                };
                spans.push(Span::styled(format!(" {} ", chip.label), style));
            }
        }
    }
    Line::from(spans)
}

/// Render a list of (key, label, primary) hints to a single help line.
/// The primary hint renders with an inverted-accent background so the
/// next-step CTA reads first. Hints overflow-truncate with "…".
pub fn help_line(hints: &[HelpHint<'_>], area_width: u16) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used: usize = 0;
    let mut truncated = false;
    for hint in hints {
        let chunk = hint.width();
        if used + chunk > area_width as usize {
            truncated = true;
            break;
        }
        used += chunk;
        match hint.primary {
            true => {
                spans.push(Span::styled(
                    format!(" {} ", hint.key),
                    Style::default()
                        .fg(BASE)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    format!(" {} ", hint.label),
                    accent().add_modifier(Modifier::BOLD),
                ));
            }
            false => {
                spans.push(Span::styled(
                    format!(" {} ", hint.key),
                    accent().add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(format!("{} ", hint.label), muted()));
            }
        }
    }
    if truncated {
        spans.push(Span::styled("…", muted()));
    }
    Line::from(spans)
}

pub struct HelpHint<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub primary: bool,
}

impl<'a> HelpHint<'a> {
    pub fn new(key: &'a str, label: &'a str) -> Self {
        Self {
            key,
            label,
            primary: false,
        }
    }

    pub fn primary(key: &'a str, label: &'a str) -> Self {
        Self {
            key,
            label,
            primary: true,
        }
    }

    fn width(&self) -> usize {
        // primary: " key " (3) + " label " (label+2)
        // plain:   " key " (key+2) + "label " (label+1)
        self.key.chars().count() + self.label.chars().count() + 3
    }
}
