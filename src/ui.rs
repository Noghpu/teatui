use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{App, Pane, View};

pub fn render(frame: &mut Frame, app: &App) {
    let [main_area, status_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [nav_area, work_area, preview_area] = Layout::horizontal([
        Constraint::Length(20),
        Constraint::Percentage(42),
        Constraint::Fill(1),
    ])
    .areas(main_area);

    render_nav(frame, app, nav_area);
    render_work(frame, app, work_area);
    render_preview(frame, app, preview_area);
    render_status(frame, app, status_area);
    render_help(frame, help_area);
}

fn render_nav(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .views()
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let content = if index == app.selected_view_index() {
                format!("> {}", view.title()).bold().cyan()
            } else {
                format!("  {}", view.title()).dim()
            };
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(focused_title(
        "teatui",
        app.focused_pane() == Pane::Navigation,
    )));

    frame.render_widget(list, area);
}

fn render_work(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let lines = match app.current_view() {
        View::Landing => vec![
            Line::from("Create Gitea PRs from jj-managed repos.".bold()),
            Line::from(""),
            Line::from(
                "This view will show repo health, detected remotes, and next actions.".dim(),
            ),
            Line::from("Use the navigation pane to switch workflows.".dim()),
        ],
        View::Generate => vec![
            Line::from("Generate PR".bold()),
            Line::from(""),
            Line::from("Collect jj status, log, descriptions, and diff.".dim()),
            Line::from("Send one complete prompt to Ollama.".dim()),
            Line::from("Review branch, title, and body before running tea.".dim()),
        ],
        View::Issues => vec![
            Line::from("Issues".bold()),
            Line::from(""),
            Line::from("List open issues, preview details, and add a simple comment.".dim()),
            Line::from("No labels, projects, milestones, or triage workflows here.".dim()),
        ],
        View::PullRequests => vec![
            Line::from("Pull Requests".bold()),
            Line::from(""),
            Line::from("List open PRs, preview details, and add a simple comment.".dim()),
            Line::from("This stays secondary to PR generation.".dim()),
        ],
        View::Logs => vec![
            Line::from("Logs".bold()),
            Line::from(""),
            Line::from("Command output and recoverable errors will appear here.".dim()),
        ],
    };

    let work = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(focused_title("Work", app.focused_pane() == Pane::Work)),
    );
    frame.render_widget(work, area);
}

fn render_preview(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let lines = match app.current_view() {
        View::Landing => vec![
            Line::from("Preview".bold()),
            Line::from(""),
            Line::from("Repo summary and recent activity will be rendered here."),
        ],
        View::Generate => vec![
            Line::from("PR Draft Preview".bold()),
            Line::from(""),
            Line::from("branch_name: feature/example".dim()),
            Line::from("title: Generated PR title".dim()),
            Line::from("body: Markdown summary, testing, risks".dim()),
        ],
        View::Issues => vec![
            Line::from("Issue Preview".bold()),
            Line::from(""),
            Line::from("Selected issue body and comments will appear here."),
        ],
        View::PullRequests => vec![
            Line::from("PR Preview".bold()),
            Line::from(""),
            Line::from("Selected PR body, status, and comments will appear here."),
        ],
        View::Logs => vec![
            Line::from("Process Output".bold()),
            Line::from(""),
            Line::from("jj, git, tea, and Ollama diagnostics will appear here."),
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
        format!(" {} ", app.current_view().title()).dim(),
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
        " h/l ".bold().cyan(),
        "focus ".dim(),
        " Enter ".bold().cyan(),
        "select ".dim(),
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
