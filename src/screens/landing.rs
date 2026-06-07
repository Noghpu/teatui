use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::domain::StatusStore;

use super::status::{render_auth, render_llm, render_tool, render_workspace};
use super::{NewScreen, Transition};

const ACTIONS: &[Action] = &[Action::GeneratePr, Action::Quit];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    GeneratePr,
    Quit,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::GeneratePr => "Generate PR",
            Action::Quit => "Quit",
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
        (KeyCode::Up, _) | (KeyCode::Char('k'), false) if state.selected > 0 => {
            state.selected -= 1;
            Transition::Dirty
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), false) if state.selected + 1 < ACTIONS.len() => {
            state.selected += 1;
            Transition::Dirty
        }
        (KeyCode::Enter, _) => match ACTIONS[state.selected] {
            Action::GeneratePr => Transition::Navigate(NewScreen::Generate),
            Action::Quit => Transition::Quit,
        },
        _ => Transition::None,
    }
}

pub fn render(state: &LandingState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" teatui ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(ACTIONS.len() as u16),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    render_actions(state.selected, frame, chunks[1]);
    render_separator(frame, chunks[3]);
    render_status(status, frame, chunks[4]);
    render_footer(frame, chunks[5]);
}

fn render_actions(selected: usize, frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = ACTIONS
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_selected = i == selected;
            let marker = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!("  {marker}{}", action.label()), style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_separator(frame: &mut Frame, area: Rect) {
    let line = Line::from(Span::styled(
        "  ────────────────────────────",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(line), area);
}

fn render_status(status: &StatusStore, frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "  status",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("    jj         {}", render_tool(&status.jj))),
        Line::from(format!("    git        {}", render_tool(&status.git))),
        Line::from(format!("    tea        {}", render_tool(&status.tea))),
        Line::from(""),
        Line::from(format!(
            "    workspace  {}",
            render_workspace(&status.workspace)
        )),
        Line::from(format!("    tea auth   {}", render_auth(&status.tea_auth))),
        Line::from(format!("    llm        {}", render_llm(&status.llm))),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let line = Line::from(Span::styled(
        "  ↑/↓ select • enter activate • q quit",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(line), area);
}
