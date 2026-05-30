use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::domain::{
    ContextBundle, ExecuteStep, GeneratedDraft, PromptBuild, RevsetSummary, Revsets, StatusStore,
};
use crate::runtime::Cached;

use super::{NewScreen, Transition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Menu,
    Form,
    Preview,
}

impl Pane {
    fn label(self) -> &'static str {
        match self {
            Pane::Menu => "menu",
            Pane::Form => "form",
            Pane::Preview => "preview",
        }
    }

    fn next(self) -> Pane {
        match self {
            Pane::Menu => Pane::Form,
            Pane::Form => Pane::Preview,
            Pane::Preview => Pane::Menu,
        }
    }

    fn prev(self) -> Pane {
        match self {
            Pane::Menu => Pane::Preview,
            Pane::Form => Pane::Menu,
            Pane::Preview => Pane::Form,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct PrForm {
    pub head: String,
    pub branch: String,
    pub base: String,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
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
    pub form: PrForm,
    pub phase: GeneratePhase,
    /// Ephemeral hint shown in Preview after a user action like
    /// copy / open. Cleared when phase changes meaningfully.
    pub last_action: Option<&'static str>,
}

impl GenerateState {
    pub fn new(default_base: String) -> Self {
        Self {
            pane: Pane::Menu,
            revset_selected: 0,
            form: PrForm {
                base: default_base,
                ..PrForm::default()
            },
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
}

pub fn on_key(state: &mut GenerateState, status: &StatusStore, key: KeyEvent) -> Transition {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, ctrl) {
        (KeyCode::Char('q'), false) | (KeyCode::Char('c'), true) => Transition::Quit,
        (KeyCode::Esc, _) => Transition::Navigate(NewScreen::Landing),
        (KeyCode::Tab, _) => {
            state.pane = state.pane.next();
            Transition::Dirty
        }
        (KeyCode::BackTab, _) => {
            state.pane = state.pane.prev();
            Transition::Dirty
        }
        (KeyCode::Char('g'), false) if !state.is_in_progress() && !state.form.head.is_empty() => {
            Transition::Generate
        }
        (KeyCode::Char('x'), false)
            if matches!(state.phase, GeneratePhase::DraftReady { .. })
                && !state.form.head.is_empty()
                && !state.form.title.is_empty()
                && !state.form.branch.is_empty()
                && !state.form.base.is_empty() =>
        {
            Transition::Execute
        }
        (KeyCode::Char('c'), false) if state.done_url().is_some() => Transition::CopyUrl,
        (KeyCode::Char('o'), false) if state.done_url().is_some() => Transition::OpenUrl,
        (KeyCode::Up, _) | (KeyCode::Char('k'), false)
            if state.pane == Pane::Menu && state.revset_selected > 0 =>
        {
            state.revset_selected -= 1;
            update_head_from_selection(state, status);
            Transition::Dirty
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), false) if state.pane == Pane::Menu => {
            let n = current_revset_count(status);
            if state.revset_selected + 1 < n {
                state.revset_selected += 1;
                update_head_from_selection(state, status);
                Transition::Dirty
            } else {
                Transition::None
            }
        }
        _ => Transition::None,
    }
}

fn current_revset_count(status: &StatusStore) -> usize {
    match status.revsets.value() {
        Some(Revsets::Loaded(items)) => items.len(),
        _ => 0,
    }
}

fn update_head_from_selection(state: &mut GenerateState, status: &StatusStore) {
    if let Some(Revsets::Loaded(items)) = status.revsets.value()
        && let Some(item) = items.get(state.revset_selected)
    {
        state.form.head = item.change_id.clone();
    }
}

pub fn render(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" teatui — generate PR ")
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
        Some(Revsets::Errored { message }) => {
            let text = format!("error: {message}");
            vec![placeholder_line(&text)]
        }
        None => match &status.revsets {
            Cached::Unknown => vec![placeholder_line("·")],
            Cached::Loading => vec![placeholder_line("loading…")],
            Cached::Stale { .. } | Cached::Ready(_) => unreachable!(),
        },
    };

    let body = Paragraph::new(lines);
    frame.render_widget(body, inner);
}

fn revset_line(
    index: usize,
    item: &RevsetSummary,
    selected: usize,
    focused: bool,
) -> Line<'static> {
    let is_selected = index == selected;
    let marker = if is_selected { "▶ " } else { "  " };
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
    let text = format!("{marker}{}{} {}", item.change_id, bookmark_tag, desc);
    Line::from(Span::styled(text, style))
}

fn placeholder_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::DarkGray),
    ))
}

fn render_form(state: &GenerateState, frame: &mut Frame, area: Rect) {
    let block = pane_block("Form", state.pane == Pane::Form);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let f = &state.form;
    let labels_str = if f.labels.is_empty() {
        "—".to_string()
    } else {
        f.labels.join(", ")
    };
    let assignees_str = if f.assignees.is_empty() {
        "—".to_string()
    } else {
        f.assignees.join(", ")
    };
    let milestone_str = fmt_or_dash(&f.milestone);
    let description_preview = if f.description.is_empty() {
        "—".to_string()
    } else {
        first_line(&f.description)
    };
    let lines = vec![
        Line::from(""),
        form_row("head", fmt_or_dash(&f.head)),
        form_row("branch", fmt_or_dash(&f.branch)),
        form_row("base", fmt_or_dash(&f.base)),
        form_row("title", fmt_or_dash(&f.title)),
        form_row("description", description_preview),
        form_row("labels", labels_str),
        form_row("assignees", assignees_str),
        form_row("milestone", milestone_str),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn fmt_or_dash(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

fn form_row(label: &str, value: String) -> Line<'static> {
    let label_span = Span::styled(
        format!("  {label:<12}"),
        Style::default().fg(Color::DarkGray),
    );
    let value_span = Span::raw(value);
    Line::from(vec![label_span, value_span])
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
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

fn preview_idle_lines(state: &GenerateState) -> Vec<Line<'static>> {
    let head = fmt_or_dash(&state.form.head);
    vec![
        Line::from(""),
        header_line("draft"),
        Line::from(""),
        hint_line("press g to generate"),
        Line::from(""),
        header_line("target"),
        Line::from(""),
        kv_line("head", head),
        kv_line("base", fmt_or_dash(&state.form.base)),
    ]
}

fn preview_collecting_lines(state: &GenerateState) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        header_line("draft"),
        Line::from(""),
        hint_line("collecting context…"),
        Line::from(""),
        kv_line("head", fmt_or_dash(&state.form.head)),
    ]
}

fn preview_generating_lines(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        header_line("draft"),
        Line::from(""),
        hint_line("generating…"),
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
        Line::from(Span::styled(
            "title",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(format!("  {}", draft.title))),
        Line::from(""),
        Line::from(Span::styled(
            "branch",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(format!("  {}", fmt_or_dash(&state.form.branch)))),
        Line::from(""),
        Line::from(Span::styled(
            "description",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    for line in draft.description.lines() {
        lines.push(Line::from(Span::raw(format!("  {line}"))));
    }
    lines.push(Line::from(""));
    lines.push(header_line("prompt"));
    lines.push(kv_line("total", fmt_bytes(prompt.manifest.total_bytes)));
    lines.push(Line::from(""));
    lines.push(hint_line("x execute • g regenerate"));
    lines
}

fn preview_executing_lines(draft: &GeneratedDraft) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        header_line("executing…"),
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
            format!("  • {}", step.label()),
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
        hint_line("c copy • o open • esc landing"),
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
    let pane_label = state.pane.label();
    let revsets_summary = match status.revsets.value() {
        Some(Revsets::Loaded(items)) => format!("{} revsets", items.len()),
        Some(Revsets::Errored { .. }) => "revsets error".to_string(),
        None => match &status.revsets {
            Cached::Loading => "revsets loading…".to_string(),
            _ => "revsets ·".to_string(),
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
    let hints = match &state.phase {
        GeneratePhase::DraftReady { .. } => "g regenerate • x execute • esc landing • q quit",
        GeneratePhase::Done { .. } => "c copy • o open • esc landing • q quit",
        _ => "g generate • tab cycle • esc landing • q quit",
    };
    let line = Line::from(vec![
        Span::styled("  pane: ", Style::default().fg(Color::DarkGray)),
        Span::styled(pane_label, Style::default().fg(Color::Cyan)),
        Span::styled("    ", Style::default()),
        Span::styled(revsets_summary, Style::default().fg(Color::DarkGray)),
        Span::styled("    phase: ", Style::default().fg(Color::DarkGray)),
        Span::styled(phase_summary, Style::default().fg(Color::Cyan)),
        Span::styled(format!("    {hints}"), Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
