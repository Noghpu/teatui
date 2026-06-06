use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::domain::{LlmHealth, StatusStore, ToolStatus, WorkspaceInfo};
use crate::runtime::Cached;

use super::theme;
use super::{NewScreen, Transition};

const ACTIONS: &[Action] = &[
    Action::GeneratePr,
    Action::ManagePrs,
    Action::ManageIssues,
    Action::Quit,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    GeneratePr,
    ManagePrs,
    ManageIssues,
    Quit,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::GeneratePr => "Generate PR",
            Action::ManagePrs => "Manage PRs",
            Action::ManageIssues => "Manage Issues",
            Action::Quit => "Quit",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Action::GeneratePr => "g",
            Action::ManagePrs => "p",
            Action::ManageIssues => "i",
            Action::Quit => "q",
        }
    }
}

#[derive(Debug, Default)]
pub struct LandingState {
    pub selected: usize,
}

pub fn on_key(state: &mut LandingState, key: KeyEvent) -> Transition {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, ctrl) {
        (KeyCode::Char('q'), false) | (KeyCode::Char('c'), true) => Transition::Quit,
        (KeyCode::Char('b'), false) => Transition::OpenBackendPicker,
        (KeyCode::Char('g'), false) => Transition::Navigate(NewScreen::Generate),
        (KeyCode::Char('p'), false) => {
            state.selected = 1;
            Transition::Dirty
        }
        (KeyCode::Char('i'), false) => {
            state.selected = 2;
            Transition::Dirty
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), false) if state.selected > 0 => {
            state.selected -= 1;
            Transition::Dirty
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), false) if state.selected + 1 < ACTIONS.len() => {
            state.selected += 1;
            Transition::Dirty
        }
        (KeyCode::Enter, _) => match ACTIONS[state.selected.min(ACTIONS.len() - 1)] {
            Action::GeneratePr => Transition::Navigate(NewScreen::Generate),
            Action::ManagePrs | Action::ManageIssues => Transition::Dirty,
            Action::Quit => Transition::Quit,
        },
        _ => Transition::None,
    }
}

pub fn render(state: &LandingState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let [main, status_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    render_landing_hero(state, status, frame, main);
    render_status_bar(state, frame, status_area);
    render_help(frame, help_area);
}

fn render_landing_hero(state: &LandingState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let [header_area, actions_area, footer_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let header_center = center_horizontally(header_area);
    let actions_center = center_horizontally(actions_area);

    let header = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "teatui",
            theme::text().add_modifier(ratatui::style::Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("jj ", theme::muted()),
            Span::raw("·"),
            Span::styled(" Forge ", theme::muted()),
            Span::raw("·"),
            Span::styled(" LLM", theme::muted()),
        ]),
        Line::from(""),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(header, header_center);
    render_landing_actions(state.selected, frame, actions_center);
    render_landing_footer(status, frame, footer_area);
}

fn center_horizontally(area: Rect) -> Rect {
    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(60),
        Constraint::Percentage(20),
    ])
    .areas(area);
    center
}

fn render_landing_actions(selected: usize, frame: &mut Frame, area: Rect) {
    let mut lines = vec![Line::from("")];
    for (i, action) in ACTIONS.iter().enumerate() {
        lines.push(landing_action_line(
            *action,
            i == selected,
            area.width as usize,
        ));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn landing_action_line(action: Action, selected: bool, center_width: usize) -> Line<'static> {
    let prefix = if selected { "▶ " } else { "  " };
    let icon = "◆";
    let key = action.key();
    let label = action.label();
    let left_len = prefix.chars().count() + icon.chars().count() + 1 + label.chars().count();
    let key_len = key.chars().count();
    let padding = " ".repeat(center_width.saturating_sub(left_len + key_len).max(1));
    if selected {
        Line::from(vec![
            Span::styled(
                format!("{prefix}{icon} {label}"),
                theme::accent().add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw(padding),
            Span::styled(key, theme::muted()),
        ])
    } else {
        Line::from(vec![
            Span::styled(format!("{prefix}{icon} "), theme::muted()),
            Span::styled(label, theme::text()),
            Span::raw(padding),
            Span::styled(key, theme::muted()),
        ])
    }
}

fn render_landing_footer(status: &StatusStore, frame: &mut Frame, area: Rect) {
    let mut spans = Vec::new();
    push_tool(&mut spans, "jj", &status.jj);
    spans.push(Span::raw("  "));
    push_tool(&mut spans, "git", &status.git);
    spans.push(Span::raw("  "));
    push_tool(&mut spans, status.forge_label.clone(), &status.forge);
    spans.push(Span::raw("  "));
    push_llm(&mut spans, status);
    spans.push(Span::raw("  "));
    push_workspace(&mut spans, status);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

fn push_tool(spans: &mut Vec<Span<'static>>, name: impl Into<String>, c: &Cached<ToolStatus>) {
    let (symbol, style) = match tool_health(c) {
        Health::Good => ("✓", theme::success()),
        Health::Bad => ("✗", theme::error()),
        Health::Warn | Health::Pending => ("·", theme::muted()),
    };
    spans.push(Span::styled(format!("{symbol} {}", name.into()), style));
}

fn push_llm(spans: &mut Vec<Span<'static>>, status: &StatusStore) {
    let (symbol, style) = match llm_health(status) {
        Health::Good => ("✓", theme::success()),
        Health::Bad => ("✗", theme::error()),
        Health::Warn | Health::Pending => ("·", theme::muted()),
    };
    let label = match status.llm.value() {
        Some(LlmHealth::Available { models }) if !models.is_empty() => {
            format!("LLM: {}", models[0])
        }
        Some(LlmHealth::Available { .. }) => "LLM: reachable".to_string(),
        Some(LlmHealth::Unreachable { .. }) => "LLM: unreachable".to_string(),
        None => "LLM: pending".to_string(),
    };
    spans.push(Span::styled(format!("{symbol} {label}"), style));
}

fn push_workspace(spans: &mut Vec<Span<'static>>, status: &StatusStore) {
    let (symbol, label, style) = match &status.workspace {
        Cached::Unknown | Cached::Loading => ("·", "discovering workspace", theme::muted()),
        Cached::Ready(WorkspaceInfo::Inside { .. })
        | Cached::Stale {
            value: WorkspaceInfo::Inside { .. },
            ..
        } => ("✓", "workspace", theme::success()),
        Cached::Ready(WorkspaceInfo::Outside)
        | Cached::Stale {
            value: WorkspaceInfo::Outside,
            ..
        } => ("·", "no jj workspace", theme::muted()),
        Cached::Ready(WorkspaceInfo::Errored { .. })
        | Cached::Stale {
            value: WorkspaceInfo::Errored { .. },
            ..
        } => ("✗", "workspace error", theme::error()),
    };
    spans.push(Span::styled(format!("{symbol} {label}"), style));
}

#[derive(Clone, Copy)]
enum Health {
    Good,
    Warn,
    Bad,
    Pending,
}

fn tool_health(c: &Cached<ToolStatus>) -> Health {
    match c {
        Cached::Unknown | Cached::Loading => Health::Pending,
        Cached::Ready(ToolStatus::Available { .. })
        | Cached::Stale {
            value: ToolStatus::Available { .. },
            ..
        } => Health::Good,
        Cached::Ready(ToolStatus::Missing)
        | Cached::Ready(ToolStatus::Errored { .. })
        | Cached::Stale {
            value: ToolStatus::Missing | ToolStatus::Errored { .. },
            ..
        } => Health::Bad,
    }
}

fn llm_health(status: &StatusStore) -> Health {
    match &status.llm {
        Cached::Unknown | Cached::Loading => Health::Pending,
        Cached::Ready(LlmHealth::Available { .. })
        | Cached::Stale {
            value: LlmHealth::Available { .. },
            ..
        } => Health::Good,
        Cached::Ready(LlmHealth::Unreachable { .. })
        | Cached::Stale {
            value: LlmHealth::Unreachable { .. },
            ..
        } => Health::Warn,
    }
}

fn render_status_bar(state: &LandingState, frame: &mut Frame, area: Rect) {
    let selected = ACTIONS[state.selected.min(ACTIONS.len() - 1)];
    let chips = vec![
        theme::StatusChip::mode("Normal"),
        theme::StatusChip::plain("Landing", 5),
        theme::StatusChip::plain(format!("selected:{}", selected.label()), 2),
    ];
    frame.render_widget(Paragraph::new(theme::status_line(chips, area.width)), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let hints = [
        theme::HelpHint::primary("Enter", "open"),
        theme::HelpHint::new("b", "backend"),
        theme::HelpHint::new("q", "quit"),
    ];
    frame.render_widget(Paragraph::new(theme::help_line(&hints, area.width)), area);
}
