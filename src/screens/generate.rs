#[path = "generate/form.rs"]
pub mod form;
#[path = "generate/input.rs"]
mod input;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::domain::{
    ContextBundle, ExecuteStep, GeneratedDraft, PromptBuild, RevsetSummary, Revsets, StatusStore,
};
use crate::runtime::Cached;

pub use self::form::{FieldId, FieldKind, FieldState, InputMode, PrForm};
use super::Transition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Menu,
    Form,
    Preview,
}

impl Pane {
    pub fn label(self) -> &'static str {
        match self {
            Pane::Menu => "menu",
            Pane::Form => "form",
            Pane::Preview => "preview",
        }
    }

    pub fn next(self) -> Pane {
        match self {
            Pane::Menu => Pane::Form,
            Pane::Form => Pane::Preview,
            Pane::Preview => Pane::Menu,
        }
    }

    pub fn prev(self) -> Pane {
        match self {
            Pane::Menu => Pane::Preview,
            Pane::Form => Pane::Menu,
            Pane::Preview => Pane::Form,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub enum GeneratePhase {
    #[default]
    Idle,
    Collecting,
    Generating {
        context: ContextBundle,
        prompt: PromptBuild,
    },
    DraftReady {
        draft: GeneratedDraft,
        prompt: PromptBuild,
    },
    Executing {
        draft: GeneratedDraft,
    },
    Done {
        url: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Default)]
pub struct GenerateState {
    pub pane: Pane,
    pub revset_selected: usize,
    pub input_mode: InputMode,
    pub field_focus: FieldId,
    pub form: PrForm,
    pub phase: GeneratePhase,
    pub last_action: Option<&'static str>,
}

impl GenerateState {
    pub fn new(default_base: String) -> Self {
        Self {
            pane: Pane::Menu,
            revset_selected: 0,
            input_mode: InputMode::Normal,
            field_focus: FieldId::Head,
            form: PrForm::new(default_base),
            phase: GeneratePhase::Idle,
            last_action: None,
        }
    }

    pub fn is_in_progress(&self) -> bool {
        matches!(
            self.phase,
            GeneratePhase::Collecting
                | GeneratePhase::Generating { .. }
                | GeneratePhase::Executing { .. }
        )
    }

    pub fn done_url(&self) -> Option<&str> {
        match &self.phase {
            GeneratePhase::Done { url } => Some(url.as_str()),
            _ => None,
        }
    }

    pub fn ensure_field_options_synced(&mut self, status: &StatusStore) {
        self.form.sync_options(status);
    }
}

pub fn on_key(state: &mut GenerateState, status: &StatusStore, key: KeyEvent) -> Transition {
    input::on_key(state, status, key)
}

pub(super) fn current_revset_count(status: &StatusStore) -> usize {
    match status.revsets.value() {
        Some(Revsets::Loaded(items)) => items.len(),
        _ => 0,
    }
}

pub(super) fn update_head_from_selection(state: &mut GenerateState, status: &StatusStore) {
    if let Some(Revsets::Loaded(items)) = status.revsets.value()
        && let Some(item) = items.get(state.revset_selected)
    {
        state.form.head.set_value(item.change_id.clone());
    }
}

pub fn render(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" teatui - generate PR ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(34),
            Constraint::Min(30),
            Constraint::Length(46),
        ])
        .split(outer[0]);

    render_menu(state, status, frame, columns[0]);
    render_form(state, frame, columns[1]);
    render_preview(state, frame, columns[2]);
    render_footer(state, status, frame, outer[1]);
    if state.input_mode == InputMode::Editing
        && let FieldState::Picker(picker) = state.form.field(state.field_focus)
    {
        render_picker_modal(state.field_focus, picker, frame, inner);
    }
}

fn render_menu(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let block = pane_block("Revsets", state.pane == Pane::Menu);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines: Vec<Line> = match status.revsets.value() {
        Some(Revsets::Loaded(items)) if !items.is_empty() => items
            .iter()
            .enumerate()
            .map(|(i, item)| revset_line(i, item, state.revset_selected, state.pane == Pane::Menu))
            .collect(),
        Some(Revsets::Loaded(_)) => vec![placeholder_line("no mutable changes")],
        Some(Revsets::Errored { message }) => vec![placeholder_line(&format!("error: {message}"))],
        None => match &status.revsets {
            Cached::Unknown => vec![placeholder_line(".")],
            Cached::Loading => vec![placeholder_line("loading...")],
            Cached::Stale { .. } | Cached::Ready(_) => unreachable!(),
        },
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

fn revset_line(
    index: usize,
    item: &RevsetSummary,
    selected: usize,
    focused: bool,
) -> Line<'static> {
    let is_selected = index == selected;
    let marker = if is_selected { "> " } else { "  " };
    let style = match (is_selected, focused) {
        (true, true) => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        (true, false) => Style::default().fg(Color::Cyan),
        (false, _) => Style::default(),
    };
    let bookmark_tag = if item.bookmarks.is_empty() {
        String::new()
    } else {
        format!(" [{}]", item.bookmarks.join(","))
    };
    let desc = if item.description.is_empty() {
        "(no description)".to_string()
    } else {
        item.description.clone()
    };
    Line::from(Span::styled(
        format!("{marker}{}{} {}", item.change_id, bookmark_tag, desc),
        style,
    ))
}

fn render_form(state: &GenerateState, frame: &mut Frame, area: Rect) {
    let block = pane_block("Form", state.pane == Pane::Form);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = vec![Line::from("")];
    for id in FieldId::ALL {
        lines.push(form_row(state, id));
        if state.input_mode == InputMode::Editing
            && state.field_focus == id
            && matches!(
                state.form.field(id).kind(),
                FieldKind::Text { multiline: true }
            )
            && let FieldState::Text(t) = state.form.field(id)
        {
            for line in t.buffer.lines() {
                lines.push(Line::from(Span::raw(format!("     {line}"))));
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn form_row(state: &GenerateState, id: FieldId) -> Line<'static> {
    let field = state.form.field(id);
    let focused = state.pane == Pane::Form && state.field_focus == id;
    let marker = if focused { "> " } else { "  " };
    let style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let value = if state.input_mode == InputMode::Editing
        && focused
        && matches!(field.kind(), FieldKind::Text { multiline: false })
        && let FieldState::Text(t) = field
    {
        fmt_or_dash(&t.buffer)
    } else {
        field_preview(field)
    };
    let dirty = if field.is_dirty() { " *" } else { "" };
    let errors = if field.errors().is_empty() {
        String::new()
    } else {
        format!("  {}", field.errors().join(", "))
    };
    Line::from(vec![
        Span::styled(
            format!("{marker}{:<12}", id.label()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{value}{dirty}"), style),
        Span::styled(errors, Style::default().fg(Color::Red)),
    ])
}

fn field_preview(field: &FieldState) -> String {
    match field {
        FieldState::Text(t) if t.multiline => {
            if t.value.is_empty() {
                "-".to_string()
            } else {
                first_line(&t.value)
            }
        }
        _ => fmt_or_dash(field.value()),
    }
}

fn render_picker_modal(
    id: FieldId,
    picker: &form::PickerFieldState,
    frame: &mut Frame,
    area: Rect,
) {
    let width = area.width.saturating_sub(8).min(64);
    let height = area.height.saturating_sub(4).min(16);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .title(format!(" {} ", id.label()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("filter: ", Style::default().fg(Color::DarkGray)),
            Span::raw(if picker.filter.is_empty() {
                "-".to_string()
            } else {
                picker.filter.clone()
            }),
        ]),
        Line::from(""),
    ];
    let visible = picker.visible_options();
    if visible.is_empty() {
        lines.push(placeholder_line("no options"));
    } else {
        for (idx, option) in visible.into_iter().enumerate() {
            let focused = idx == picker.highlighted;
            let marker = if picker.multi_select {
                if picker.draft_contains(&option.value) {
                    "[x] "
                } else {
                    "[ ] "
                }
            } else if focused {
                "> "
            } else {
                "  "
            };
            let style = if focused {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", option.label),
                style,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_preview(state: &GenerateState, frame: &mut Frame, area: Rect) {
    let block = pane_block("Preview", state.pane == Pane::Preview);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = match &state.phase {
        GeneratePhase::Idle => preview_idle_lines(state),
        GeneratePhase::Collecting => preview_collecting_lines(state),
        GeneratePhase::Generating { prompt, .. } => preview_generating_lines(prompt),
        GeneratePhase::DraftReady { draft, prompt } => preview_draft_lines(state, draft, prompt),
        GeneratePhase::Executing { draft } => preview_executing_lines(draft),
        GeneratePhase::Done { url } => preview_done_lines(url),
        GeneratePhase::Failed { message } => preview_failed_lines(message),
    };
    if let Some(hint) = state.last_action {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {hint}"),
            Style::default().fg(Color::Green),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn preview_idle_lines(state: &GenerateState) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        header_line("draft"),
        Line::from(""),
        hint_line("press g to generate"),
        Line::from(""),
        header_line("target"),
        Line::from(""),
        kv_line("head", fmt_or_dash(state.form.head())),
        kv_line("base", fmt_or_dash(state.form.base())),
    ]
}

fn preview_collecting_lines(state: &GenerateState) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        header_line("draft"),
        Line::from(""),
        hint_line("collecting context..."),
        Line::from(""),
        kv_line("head", fmt_or_dash(state.form.head())),
    ]
}

fn preview_generating_lines(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        header_line("draft"),
        Line::from(""),
        hint_line("generating..."),
        Line::from(""),
        header_line("prompt manifest"),
        Line::from(""),
    ];
    for section in &prompt.manifest.sections {
        lines.push(kv_line(section.name, fmt_bytes(section.bytes)));
    }
    lines.push(Line::from(""));
    lines.push(kv_line("total", fmt_bytes(prompt.manifest.total_bytes)));
    lines
}

fn preview_draft_lines(
    state: &GenerateState,
    draft: &GeneratedDraft,
    prompt: &PromptBuild,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        header_line("draft ready"),
        Line::from(""),
        header_line("title"),
        Line::from(Span::raw(format!("  {}", draft.title))),
        Line::from(""),
        header_line("branch"),
        Line::from(Span::raw(format!("  {}", fmt_or_dash(state.form.branch())))),
        Line::from(""),
        header_line("description"),
    ];
    for line in draft.description.lines() {
        lines.push(Line::from(Span::raw(format!("  {line}"))));
    }
    lines.push(Line::from(""));
    lines.push(header_line("prompt"));
    lines.push(kv_line("total", fmt_bytes(prompt.manifest.total_bytes)));
    lines.push(Line::from(""));
    lines.push(hint_line("x execute - g regenerate"));
    lines
}

fn preview_executing_lines(draft: &GeneratedDraft) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        header_line("executing..."),
        Line::from(""),
        Line::from(Span::raw(format!("  title: {}", draft.title))),
        Line::from(""),
        header_line("steps"),
        Line::from(""),
    ];
    for step in [
        ExecuteStep::Bookmark,
        ExecuteStep::Push,
        ExecuteStep::Create,
    ] {
        lines.push(Line::from(Span::styled(
            format!("  - {}", step.label()),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}

fn preview_done_lines(url: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "done",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(format!("  {url}"))),
        Line::from(""),
        hint_line("c copy - o open - esc landing"),
    ]
}

fn preview_failed_lines(message: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "failed",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(format!("  {message}"))),
        Line::from(""),
        hint_line("press g to retry"),
    ]
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn fmt_or_dash(s: &str) -> String {
    if s.is_empty() {
        "-".to_string()
    } else {
        s.to_string()
    }
}

fn placeholder_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::DarkGray),
    ))
}

fn header_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    ))
}

fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {text}"),
        Style::default().fg(Color::DarkGray),
    ))
}

fn kv_line(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<10}"), Style::default().fg(Color::DarkGray)),
        Span::raw(value),
    ])
}

fn fmt_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / 1024.0 / 1024.0)
    }
}

fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(style)
}

fn render_footer(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let revsets_summary = match status.revsets.value() {
        Some(Revsets::Loaded(items)) => format!("{} revsets", items.len()),
        Some(Revsets::Errored { .. }) => "revsets error".to_string(),
        None => match &status.revsets {
            Cached::Loading => "revsets loading...".to_string(),
            _ => "revsets .".to_string(),
        },
    };
    let phase_summary = match &state.phase {
        GeneratePhase::Idle => "idle",
        GeneratePhase::Collecting => "collecting",
        GeneratePhase::Generating { .. } => "generating",
        GeneratePhase::DraftReady { .. } => "draft",
        GeneratePhase::Executing { .. } => "executing",
        GeneratePhase::Done { .. } => "done",
        GeneratePhase::Failed { .. } => "failed",
    };
    let hints = if state.input_mode == InputMode::Editing {
        match state.form.field(state.field_focus).kind() {
            FieldKind::Text { multiline: true } => "ctrl-s commit - esc cancel",
            FieldKind::Text { multiline: false } => "enter commit - esc cancel",
            FieldKind::Picker {
                multi_select: true, ..
            } => "enter commit - space toggle - esc cancel",
            FieldKind::Picker { .. } => "enter commit - esc cancel",
        }
    } else {
        match &state.phase {
            GeneratePhase::DraftReady { .. } => {
                "i edit - g regenerate - x execute - tab cycle - esc landing"
            }
            GeneratePhase::Done { .. } => "c copy - o open - esc landing - q quit",
            _ => "i edit - g generate - tab cycle - esc landing - q quit",
        }
    };
    let line = Line::from(vec![
        Span::styled("  pane: ", Style::default().fg(Color::DarkGray)),
        Span::styled(state.pane.label(), Style::default().fg(Color::Cyan)),
        Span::styled("    field: ", Style::default().fg(Color::DarkGray)),
        Span::styled(state.field_focus.label(), Style::default().fg(Color::Cyan)),
        Span::styled("    ", Style::default()),
        Span::styled(revsets_summary, Style::default().fg(Color::DarkGray)),
        Span::styled("    phase: ", Style::default().fg(Color::DarkGray)),
        Span::styled(phase_summary, Style::default().fg(Color::Cyan)),
        Span::styled(format!("    {hints}"), Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
