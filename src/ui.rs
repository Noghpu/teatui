use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{App, Pane, Screen};

pub fn render(frame: &mut Frame, app: &App) {
    let [main_area, status_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [menu_area, form_area, preview_area] = Layout::horizontal([
        Constraint::Length(28),
        Constraint::Percentage(42),
        Constraint::Fill(1),
    ])
    .areas(main_area);

    render_menu(frame, app, menu_area);
    render_work(frame, app, form_area);
    render_preview(frame, app, preview_area);
    render_status(frame, app, status_area);
    render_help(frame, help_area);
}

fn render_menu(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = match app.screen() {
        Screen::Landing => app
            .landing_entries()
            .iter()
            .enumerate()
            .map(|(index, screen)| {
                let label = format!(
                    "{} {}",
                    if index == app.selected_landing_entry_index() {
                        ">"
                    } else {
                        " "
                    },
                    screen.title()
                );
                if index == app.selected_landing_entry_index() {
                    ListItem::new(label.bold().cyan())
                } else {
                    ListItem::new(label.dim())
                }
            })
            .collect(),
        Screen::Generate => app
            .revsets()
            .iter()
            .enumerate()
            .map(|(index, revset)| {
                let bookmarks = if revset.bookmarks().is_empty() {
                    "no bookmark".to_string()
                } else {
                    revset.bookmarks().join(", ")
                };
                let label = format!("{}  {}", revset.label(), bookmarks);
                if index == app.selected_revset_index() {
                    ListItem::new(label.bold().cyan())
                } else {
                    ListItem::new(label.dim())
                }
            })
            .collect(),
        Screen::PullRequests | Screen::Issues => app
            .secondary_items()
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let label = format!(
                    "{} {}",
                    if index == app.selected_secondary_item_index() {
                        ">"
                    } else {
                        " "
                    },
                    item
                );
                if index == app.selected_secondary_item_index() {
                    ListItem::new(label.bold().cyan())
                } else {
                    ListItem::new(label.dim())
                }
            })
            .collect(),
    };

    let title = match app.screen() {
        Screen::Landing => "Modes",
        Screen::Generate => "Revsets",
        Screen::PullRequests => "PRs",
        Screen::Issues => "Issues",
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(focused_title(title, app.focused_pane() == Pane::Menu)),
    );

    frame.render_widget(list, area);
}

fn render_work(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let lines = match app.screen() {
        Screen::Landing => vec![
            Line::from("teatui".bold()),
            Line::from(""),
            Line::from("jj workspace: detected".dim()),
            Line::from("Gitea remote: pending".dim()),
            Line::from("tea auth: pending".dim()),
            Line::from("Ollama: pending".dim()),
            Line::from(""),
            Line::from("Select a mode on the left.".dim()),
        ],
        Screen::Generate => app
            .form_fields()
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let value = match *field {
                    "head" => app.selected_revset().label().to_string(),
                    "branch name" => app
                        .selected_revset()
                        .bookmarks()
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    "base" => "main@origin".to_string(),
                    "title" | "description" => String::new(),
                    "labels" | "assignees" | "milestone" => "optional".to_string(),
                    _ => String::new(),
                };
                let marker = if index == app.selected_field_index() {
                    ">"
                } else {
                    " "
                };
                let line = format!("{marker} {field}: {value}");
                if index == app.selected_field_index() {
                    Line::from(line.bold().cyan())
                } else {
                    Line::from(line.dim())
                }
            })
            .collect(),
        Screen::PullRequests => vec![
            Line::from("Manage PRs".bold()),
            Line::from(""),
            Line::from("List open PRs, preview details, and add a simple comment.".dim()),
            Line::from("This mode stays intentionally small.".dim()),
        ],
        Screen::Issues => vec![
            Line::from("Manage Issues".bold()),
            Line::from(""),
            Line::from("List open issues, preview details, and add a simple comment.".dim()),
            Line::from("This mode stays intentionally small.".dim()),
        ],
    };

    let title = match app.screen() {
        Screen::Landing => "Status",
        Screen::Generate => "PR Form",
        Screen::PullRequests | Screen::Issues => "Work",
    };

    let form = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(focused_title(title, app.focused_pane() == Pane::Form)),
    );
    frame.render_widget(form, area);
}

fn render_preview(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let lines = match app.screen() {
        Screen::Landing => vec![
            Line::from("Landing".bold()),
            Line::from(""),
            Line::from("Generate PR, Manage PRs, and Manage Issues are separate modes."),
            Line::from("Press Enter to open the selected mode.".dim()),
        ],
        Screen::Generate => {
            let revset = app.selected_revset();
            vec![
                Line::from("Selected Revset".bold()),
                Line::from(""),
                Line::from(format!("revset: {}", revset.label()).cyan()),
                Line::from(format!("description: {}", revset.description())),
                Line::from(format!("bookmarks: {}", revset.bookmarks().join(", ")).dim()),
                Line::from(format!("stats: {}", revset.stats()).dim()),
                Line::from(""),
                Line::from("Press Enter on the revset list to move to the PR form.".dim()),
                Line::from("Press g from navigation mode to generate using all form values.".dim()),
            ]
        }
        Screen::PullRequests => vec![
            Line::from("PR Preview".bold()),
            Line::from(""),
            Line::from("Selected PR body, status, and comments will appear here."),
            Line::from("Esc returns to Landing.".dim()),
        ],
        Screen::Issues => vec![
            Line::from("Issue Preview".bold()),
            Line::from(""),
            Line::from("Selected issue body and comments will appear here."),
            Line::from("Esc returns to Landing.".dim()),
        ],
    };

    let preview = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(
        focused_title("Preview", app.focused_pane() == Pane::Preview),
    ));
    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let status = Line::from(vec![
        " NORMAL ".bold().on_cyan(),
        format!(" {} ", app.screen().title()).dim(),
        " jj + tea + ollama ".dim(),
    ]);
    frame.render_widget(Paragraph::new(status), area);
}

fn render_help(frame: &mut Frame, area: ratatui::layout::Rect) {
    let help = Line::from(vec![
        " ↑/k ".bold().cyan(),
        "up ".dim(),
        " ↓/j ".bold().cyan(),
        "down ".dim(),
        " Enter ".bold().cyan(),
        "select/edit ".dim(),
        " i ".bold().cyan(),
        "edit ".dim(),
        " g ".bold().cyan(),
        "generate ".dim(),
        " Esc ".bold().cyan(),
        "back ".dim(),
        " q ".bold().cyan(),
        "quit ".dim(),
    ]);
    frame.render_widget(Paragraph::new(help), area);
}

fn focused_title(title: &'static str, focused: bool) -> Line<'static> {
    if focused {
        title.bold().cyan().into()
    } else {
        title.dim().into()
    }
}
