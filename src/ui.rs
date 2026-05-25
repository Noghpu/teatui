use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{App, Screen};
use crate::generate::{FORM_FIELDS, Focus};

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
        Screen::Landing => ["Generate PR", "Manage PRs", "Manage Issues"]
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let marker = if index == app.landing().selected_entry {
                    ">"
                } else {
                    " "
                };
                let line = format!("{marker} {label}");
                if index == app.landing().selected_entry {
                    ListItem::new(line.bold().cyan())
                } else {
                    ListItem::new(line.dim())
                }
            })
            .collect(),
        Screen::Generate => app
            .generate()
            .revsets
            .iter()
            .enumerate()
            .map(|(index, revset)| {
                let bookmarks = if revset.bookmarks().is_empty() {
                    "no bookmark".to_string()
                } else {
                    revset.bookmarks().join(", ")
                };
                let label = format!("{}  {}", revset.label(), bookmarks);
                if index == app.generate().selected_revset {
                    ListItem::new(label.bold().cyan())
                } else {
                    ListItem::new(label.dim())
                }
            })
            .collect(),
        Screen::PullRequests => ["Open items", "Filter", "Comment"]
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let marker = if index == app.pull_requests().selected_item {
                    ">"
                } else {
                    " "
                };
                let line = format!("{marker} {item}");
                if index == app.pull_requests().selected_item {
                    ListItem::new(line.bold().cyan())
                } else {
                    ListItem::new(line.dim())
                }
            })
            .collect(),
        Screen::Issues => ["Open items", "Filter", "Comment"]
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let marker = if index == app.issues().selected_item {
                    ">"
                } else {
                    " "
                };
                let line = format!("{marker} {item}");
                if index == app.issues().selected_item {
                    ListItem::new(line.bold().cyan())
                } else {
                    ListItem::new(line.dim())
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
            .title(focused_title(title, app.focus() == Focus::Menu)),
    );

    frame.render_widget(list, area);
}

fn render_work(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let lines = match app.screen() {
        Screen::Landing => vec![
            Line::from("teatui".bold()),
            Line::from(""),
            Line::from(match &app.repo().workspace_root {
                Some(path) => format!("Workspace: {}", path.display()),
                None => "Workspace: pending".to_string(),
            }),
            status_line("Workspace", app.repo().inside_workspace, "detected"),
            status_line("Gitea remote", app.repo().gitea_remote.is_some(), "pending"),
            status_line(
                "tea auth",
                app.repo().tea_authenticated.unwrap_or(false),
                "pending",
            ),
            status_line(
                "Ollama",
                app.repo().ollama_reachable.unwrap_or(false),
                "pending",
            ),
            Line::from(match &app.repo().base_branch {
                Some(branch) => format!("Base branch: {}", branch),
                None => "Base branch: pending".to_string(),
            }),
            Line::from(format!("Logs: {}", app.logs().entries.len())),
            Line::from(""),
            Line::from("Select a mode on the left.".dim()),
        ],
        Screen::Generate => FORM_FIELDS
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let generate = app.generate();
                let value = match *field {
                    "head" => generate.form.head.display_value().to_string(),
                    "branch name" => generate.form.branch_name.display_value().to_string(),
                    "base" => generate.form.base.display_value().to_string(),
                    "title" => generate.form.title.display_value().to_string(),
                    "description" => generate.form.description.display_value().to_string(),
                    "labels" => generate.form.labels.display_value().to_string(),
                    "assignees" => generate.form.assignees.display_value().to_string(),
                    "milestone" => generate.form.milestone.display_value().to_string(),
                    _ => String::new(),
                };
                let error_count = match *field {
                    "head" => generate.form.head.errors.len(),
                    "branch name" => generate.form.branch_name.errors.len(),
                    "base" => generate.form.base.errors.len(),
                    "title" => generate.form.title.errors.len(),
                    "description" => generate.form.description.errors.len(),
                    "labels" => generate.form.labels.errors.len(),
                    "assignees" => generate.form.assignees.errors.len(),
                    "milestone" => generate.form.milestone.errors.len(),
                    _ => 0,
                };
                let marker = if index == generate.selected_field {
                    ">"
                } else {
                    " "
                };
                let line = if error_count > 0 {
                    format!("{marker} {field}: {value} ({error_count} errors)")
                } else {
                    format!("{marker} {field}: {value}")
                };
                if index == generate.selected_field && app.focus() == Focus::Form {
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
            .title(focused_title(title, app.focus() == Focus::Form)),
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
            let revset = app.generate().selected_revset();
            let mut lines = vec![
                Line::from("Selected Revset".bold()),
                Line::from(""),
                Line::from(format!("revset: {}", revset.label()).cyan()),
                Line::from(format!("description: {}", revset.description())),
                Line::from(format!("bookmarks: {}", revset.bookmarks().join(", ")).dim()),
                Line::from(format!("stats: {}", revset.stats()).dim()),
                Line::from(""),
                Line::from(format!("phase: {:?}", app.generate().phase).dim()),
                Line::from(format!("input mode: {:?}", app.generate().input_mode).dim()),
                Line::from(format!(
                    "focused field: {}",
                    app.generate().selected_field_name()
                )),
            ];

            if let Some(draft) = &app.generate().draft {
                lines.push(Line::from(""));
                lines.push(Line::from("Draft".bold()));
                lines.push(Line::from(format!("branch: {}", draft.branch_name)));
                lines.push(Line::from(format!("title: {}", draft.title)));
                lines.push(Line::from(format!("body chars: {}", draft.body.len())).dim());
                lines.push(Line::from(format!(
                    "review notes: {}",
                    draft.review_notes.len()
                )));
                lines.push(Line::from(format!(
                    "raw response chars: {}",
                    draft.raw_model_response.len()
                )));
            }

            lines.push(Line::from(format!(
                "review summary: {}",
                app.generate().review.summary
            )));
            lines.push(Line::from(format!(
                "review notes: {}",
                app.generate().review.notes.len()
            )));
            lines.push(Line::from(format!(
                "review warnings: {}",
                app.generate().review.warnings.len()
            )));

            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press Enter on the revset list to move to the PR form.".dim(),
            ));
            lines.push(Line::from(
                "Press g from navigation mode to generate using all form values.".dim(),
            ));
            lines
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

    let preview = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(focused_title("Preview", app.focus() == Focus::Preview)),
    );
    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let status = Line::from(vec![
        format!(" {} ", format!("{:?}", app.input_mode()).to_uppercase())
            .bold()
            .on_cyan(),
        format!(" {} ", app.screen().title()).dim(),
        format!(" job:{:?} ", app.jobs().status).dim(),
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

fn status_line(label: &'static str, active: bool, fallback: &'static str) -> Line<'static> {
    if active {
        Line::from(format!("{label}: detected"))
    } else {
        Line::from(format!("{label}: {fallback}")).dim()
    }
}
