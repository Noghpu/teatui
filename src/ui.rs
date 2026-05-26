use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, Screen};
use crate::generate::{FieldId, Focus, GeneratePhase, GenerateState, PromptView};
use crate::prompt::PromptBuild;
use crate::repo::{OllamaStatus, TeaAuth, ToolStatus};

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
    render_help(frame, app, help_area);
}

fn render_menu(frame: &mut Frame, app: &App, area: Rect) {
    let (items, title): (Vec<ListItem>, &'static str) = match app.screen() {
        Screen::Landing => (
            selectable_list(
                &["Generate PR", "Manage PRs", "Manage Issues"],
                app.landing().selected_entry,
            ),
            "Modes",
        ),
        Screen::Generate => (
            app.generate()
                .revsets
                .iter()
                .enumerate()
                .map(|(index, revset)| {
                    let bookmarks = if revset.bookmarks().is_empty() {
                        String::new()
                    } else {
                        revset.bookmarks().join(", ")
                    };
                    let label = if bookmarks.is_empty() {
                        format!("{}  {} commits", revset.label(), revset.commit_count())
                    } else {
                        format!(
                            "{}  {} commits  {}",
                            revset.label(),
                            revset.commit_count(),
                            bookmarks
                        )
                    };
                    list_item(&label, index == app.generate().selected_revset)
                })
                .collect(),
            "Revsets",
        ),
        Screen::PullRequests => (
            selectable_list(
                &["Open items", "Filter", "Comment"],
                app.pull_requests().selected_item,
            ),
            "PRs",
        ),
        Screen::Issues => (
            selectable_list(
                &["Open items", "Filter", "Comment"],
                app.issues().selected_item,
            ),
            "Issues",
        ),
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(focused_title(title, app.focus() == Focus::Menu)),
    );

    frame.render_widget(list, area);
}

fn selectable_list(labels: &[&str], selected: usize) -> Vec<ListItem<'static>> {
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let marker = if index == selected { ">" } else { " " };
            let line = format!("{marker} {label}");
            list_item(&line, index == selected)
        })
        .collect()
}

fn list_item(text: &str, selected: bool) -> ListItem<'static> {
    if selected {
        ListItem::new(text.to_string().bold().cyan())
    } else {
        ListItem::new(text.to_string().dim())
    }
}

fn render_work(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.screen() {
        Screen::Landing => {
            let repo = app.repo();
            let mut lines = vec![
                Line::from("teatui".bold()),
                Line::from(""),
                Line::from(match &repo.workspace_root {
                    Some(path) => format!("Workspace: {}", path.display()),
                    None => "Workspace: pending".to_string(),
                }),
                status_line("jj", repo.jj.label(), tool_tone(&repo.jj)),
                status_line("git", repo.git.label(), tool_tone(&repo.git)),
                status_line("tea", repo.tea.label(), tool_tone(&repo.tea)),
                status_line(
                    "Workspace",
                    if repo.inside_workspace {
                        "detected"
                    } else {
                        "missing"
                    },
                    if repo.inside_workspace {
                        StatusTone::Good
                    } else {
                        StatusTone::Muted
                    },
                ),
            ];

            lines.push(status_line(
                "Gitea host",
                repo.remote
                    .as_ref()
                    .map(|remote| remote.host.as_str())
                    .filter(|host| !host.is_empty())
                    .unwrap_or("(not configured)"),
                if repo
                    .remote
                    .as_ref()
                    .map(|remote| !remote.host.is_empty())
                    .unwrap_or(false)
                {
                    StatusTone::Good
                } else {
                    StatusTone::Muted
                },
            ));

            if let Some(remote) = &repo.remote {
                lines.push(Line::from(format!("Remote URL: {}", remote.raw_url)).dim());
                if let Some(warning) = &remote.warning {
                    lines.push(Line::from(format!("Remote warning: {warning}")).yellow());
                }
            }

            lines.push(status_line(
                "Tea auth",
                repo.tea_auth.label(),
                match &repo.tea_auth {
                    TeaAuth::Configured { .. } => StatusTone::Good,
                    TeaAuth::Error(_) => StatusTone::Bad,
                    TeaAuth::NotConfigured | TeaAuth::Unknown(_) => StatusTone::Muted,
                },
            ));
            if let Some(detail) = repo.tea_auth.detail() {
                lines.push(match &repo.tea_auth {
                    TeaAuth::Error(_) => Line::from(detail.to_string()).red(),
                    _ => Line::from(detail.to_string()).dim(),
                });
            }
            if let TeaAuth::Configured { host, user } = &repo.tea_auth {
                lines.push(Line::from(format!("Tea host: {host}")).green());
                if let Some(user) = user {
                    lines.push(Line::from(format!("Tea user: {user}")).green());
                }
            }

            lines.push(status_line(
                "Ollama",
                repo.ollama.label(),
                match &repo.ollama {
                    OllamaStatus::Reachable => StatusTone::Good,
                    OllamaStatus::Unreachable(_) => StatusTone::Bad,
                    OllamaStatus::Unknown(_) => StatusTone::Muted,
                },
            ));
            lines.push(Line::from(format!("Ollama endpoint: {}", repo.ollama_base_url)).dim());
            lines.push(Line::from(format!("Ollama model: {}", repo.ollama_model)).dim());
            if let Some(detail) = repo.ollama.detail() {
                lines.push(match &repo.ollama {
                    OllamaStatus::Unreachable(_) => Line::from(detail.to_string()).red(),
                    _ => Line::from(detail.to_string()).dim(),
                });
            }

            lines.push(Line::from(format!(
                "Base branch: {}",
                repo.base_branch.name
            )));
            lines.push(Line::from(format!("Logs: {}", app.logs().entries.len())).dim());
            lines.push(Line::from(""));
            lines.push(Line::from("Select a mode on the left.".dim()));
            lines
        }
        Screen::Generate => FieldId::ALL
            .iter()
            .enumerate()
            .flat_map(|(index, field_id)| {
                render_generate_field(
                    app.generate(),
                    *field_id,
                    index == app.generate().selected_field,
                    index == app.generate().selected_field && app.focus() == Focus::Form,
                )
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
        Screen::Generate => {
            if app.generate().phase == GeneratePhase::DraftReady {
                "Draft Review"
            } else {
                "PR Form"
            }
        }
        Screen::PullRequests | Screen::Issues => "Work",
    };

    let form = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(focused_title(title, app.focus() == Focus::Form)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(form, area);
}

fn render_preview(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.screen() {
        Screen::Landing => {
            let mut lines = vec![
                Line::from("Landing".bold()),
                Line::from(""),
                Line::from("Generate PR, Manage PRs, and Manage Issues are separate modes."),
                Line::from("Press Enter to open the selected mode.".dim()),
            ];

            let blockers = app.repo().blocker_lines();
            if !blockers.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from("Setup blockers".bold()));
                for blocker in blockers {
                    lines.push(Line::from(format!("- {blocker}")).red());
                }
            }

            lines
        }
        Screen::Generate => render_generate_preview(app),
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

    let preview = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(focused_title("Preview", app.focus() == Focus::Preview)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let focus = match app.focus() {
        Focus::Menu => "focus:menu",
        Focus::Form => "focus:form",
        Focus::Preview => "focus:preview",
    };

    let mut segments = vec![
        format!(" {} ", app.input_mode().label()).bold().on_cyan(),
        format!(" {} ", app.screen().title()).dim(),
        format!(" {focus} ").dim(),
    ];

    if app.screen() == Screen::Generate {
        segments.push(format!(" phase:{} ", app.generate().phase.label()).dim());
        let prompt_mode = match app.generate().prompt_view {
            PromptView::Manifest => "prompt:manifest",
            PromptView::Prompt => "prompt:text",
        };
        segments.push(format!(" {prompt_mode} ").dim());
    }

    frame.render_widget(Paragraph::new(Line::from(segments)), area);
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.screen() {
        Screen::Landing => Line::from(vec![
            " ↑/k ".bold().cyan(),
            "up ".dim(),
            " ↓/j ".bold().cyan(),
            "down ".dim(),
            " Enter ".bold().cyan(),
            "open ".dim(),
            " Esc ".bold().cyan(),
            "back ".dim(),
            " q ".bold().cyan(),
            "quit ".dim(),
        ]),
        Screen::Generate if app.input_mode() == crate::generate::InputMode::Editing => {
            Line::from(vec![
                " typing ".bold().cyan(),
                "edit field ".dim(),
                " Enter ".bold().cyan(),
                "save ".dim(),
                " Esc ".bold().cyan(),
                "cancel ".dim(),
            ])
        }
        Screen::Generate if app.focus() == Focus::Preview => Line::from(vec![
            " p ".bold().cyan(),
            "toggle prompt ".dim(),
            " g ".bold().cyan(),
            "regenerate ".dim(),
            " Esc ".bold().cyan(),
            "back ".dim(),
        ]),
        Screen::Generate => Line::from(vec![
            " ↑/k ".bold().cyan(),
            "up ".dim(),
            " ↓/j ".bold().cyan(),
            "down ".dim(),
            " h/l ".bold().cyan(),
            "move focus ".dim(),
            " Enter ".bold().cyan(),
            "select/edit ".dim(),
            " i ".bold().cyan(),
            "edit ".dim(),
            " g ".bold().cyan(),
            "generate ".dim(),
            " p ".bold().cyan(),
            "prompt ".dim(),
            " r ".bold().cyan(),
            "refresh ".dim(),
            " Esc ".bold().cyan(),
            "back ".dim(),
        ]),
        Screen::PullRequests | Screen::Issues => Line::from(vec![
            " ↑/k ".bold().cyan(),
            "up ".dim(),
            " ↓/j ".bold().cyan(),
            "down ".dim(),
            " Enter ".bold().cyan(),
            "select ".dim(),
            " c ".bold().cyan(),
            "comment ".dim(),
            " Esc ".bold().cyan(),
            "back ".dim(),
        ]),
    };
    frame.render_widget(Paragraph::new(help), area);
}

fn focused_title(title: &'static str, focused: bool) -> Line<'static> {
    if focused {
        title.bold().cyan().into()
    } else {
        title.dim().into()
    }
}

#[derive(Clone, Copy)]
enum StatusTone {
    Good,
    Muted,
    Bad,
}

fn tool_tone(status: &ToolStatus) -> StatusTone {
    match status {
        ToolStatus::Available => StatusTone::Good,
        ToolStatus::Missing | ToolStatus::Unknown => StatusTone::Muted,
        ToolStatus::Error(_) => StatusTone::Bad,
    }
}

fn status_line(label: &str, value: impl Into<String>, tone: StatusTone) -> Line<'static> {
    let line = Line::from(format!("{label}: {}", value.into()));
    match tone {
        StatusTone::Good => line.green(),
        StatusTone::Muted => line.dim(),
        StatusTone::Bad => line.red(),
    }
}

fn render_prompt_manifest(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let manifest = &prompt.manifest;
    let mut lines = vec![
        Line::from("Prompt manifest".bold()),
        Line::from(""),
        Line::from(format!("selected revset: {}", manifest.selected_revset).cyan()),
        Line::from(format!("base branch: {}", manifest.base_branch)),
        Line::from(format!("prompt bytes: {}", manifest.byte_count)).dim(),
        Line::from(format!(
            "included sections: {}",
            manifest.included_sections.len()
        )),
        Line::from(format!(
            "omitted sections: {}",
            manifest.omitted_sections.len()
        )),
    ];

    lines.push(Line::from(""));
    lines.push(Line::from("Form values".bold()));
    lines.push(Line::from(format!("head: {}", manifest.form_values.head)));
    lines.push(Line::from(format!(
        "branch name: {}",
        manifest.form_values.branch_name
    )));
    lines.push(Line::from(format!("base: {}", manifest.form_values.base)));
    lines.push(Line::from(format!("title: {}", manifest.form_values.title)));
    lines.push(Line::from(format!(
        "description: {}",
        manifest.form_values.description
    )));
    lines.push(Line::from(format!(
        "labels: {}",
        manifest.form_values.labels
    )));
    lines.push(Line::from(format!(
        "assignees: {}",
        manifest.form_values.assignees
    )));
    lines.push(Line::from(format!(
        "milestone: {}",
        manifest.form_values.milestone
    )));

    if !manifest.truncation_warnings.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("Truncation warnings".bold()));
        for warning in &manifest.truncation_warnings {
            lines.push(Line::from(warning.clone()).yellow());
        }
    }

    if !manifest.included_sections.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("Included sections".bold()));
        for section in &manifest.included_sections {
            let marker = if section.truncated {
                " [truncated]"
            } else {
                ""
            };
            lines.push(Line::from(format!(
                "- {} ({} bytes{})",
                section.title, section.byte_count, marker
            )));
        }
    }

    if !manifest.omitted_sections.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("Omitted sections".bold()));
        for section in &manifest.omitted_sections {
            lines.push(
                Line::from(format!(
                    "- {}: {} ({} bytes)",
                    section.title, section.reason, section.byte_count
                ))
                .red(),
            );
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Press p to view the full prompt text.".dim()));
    lines
}

fn render_prompt_text(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Prompt text".bold()), Line::from("")];

    for line in prompt.prompt.lines() {
        lines.push(Line::from(line.to_string()));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Press p to return to the manifest.".dim()));
    lines
}

fn render_generate_field(
    generate: &GenerateState,
    field_id: FieldId,
    selected: bool,
    focused: bool,
) -> Vec<Line<'static>> {
    let field = generate.form.field(field_id);
    let label = field_id.label();
    let value = field.display_value().to_string();
    let error_count = field.errors.len();
    let marker = if selected { ">" } else { " " };
    let header = if matches!(field_id, FieldId::Description) {
        if error_count > 0 {
            format!("{marker} {label} ({error_count} errors)")
        } else {
            format!("{marker} {label}")
        }
    } else if error_count > 0 {
        format!("{marker} {label}: {value} ({error_count} errors)")
    } else {
        format!("{marker} {label}: {value}")
    };

    let mut lines =
        Vec::with_capacity(1 + error_count + usize::from(matches!(field_id, FieldId::Description)));
    if focused {
        lines.push(Line::from(header.bold().cyan()));
    } else {
        lines.push(Line::from(header.dim()));
    }

    if matches!(field_id, FieldId::Description) {
        if value.trim().is_empty() {
            lines.push(Line::from("    (empty)").dim());
        } else {
            for line in value.lines() {
                lines.push(Line::from(format!("    {line}")));
            }
        }
    }

    for error in &field.errors {
        lines.push(Line::from(format!("    - {error}")).red());
    }

    lines
}

fn render_generate_preview(app: &App) -> Vec<Line<'static>> {
    let generate = app.generate();
    let revset = generate.selected_revset();
    let mut lines = vec![
        Line::from("Selected Revset".bold()),
        Line::from(""),
        Line::from(format!("revset: {}", revset.label()).cyan()),
        Line::from(format!("description: {}", revset.description())),
        Line::from(format!("bookmarks: {}", revset.bookmarks().join(", ")).dim()),
        Line::from(format!("stats: {}", revset.stats()).dim()),
        Line::from(format!("commits: {}", revset.commit_count())),
        Line::from(format!("commit ids: {}", revset.commit_ids().join(", "))),
        Line::from(format!("change ids: {}", revset.change_ids().join(", "))),
        Line::from(""),
        Line::from(format!("phase: {}", generate.phase.label()).dim()),
        Line::from(format!("input mode: {}", app.input_mode().label()).dim()),
        Line::from(format!("focused field: {}", generate.selected_field_name())),
        Line::from(format!(
            "base branch: {} ({:?})",
            app.repo().base_branch.name,
            app.repo().base_branch.source
        )),
    ];

    match generate.phase {
        GeneratePhase::CollectingContext => {
            lines.push(Line::from(""));
            lines.push(Line::from("Collecting context".bold()));
            lines.push(Line::from(format!(
                "selected revset: {}",
                generate.selected_revset().label()
            )));
            lines.push(Line::from(format!(
                "base branch: {}",
                generate.form.base.display_value()
            )));
            lines.push(Line::from("jj status".dim()));
            lines.push(Line::from("jj log".dim()));
            lines.push(Line::from("jj diff --stat".dim()));
            lines.push(Line::from("jj diff".dim()));
        }
        GeneratePhase::Generating => {
            lines.push(Line::from(""));
            lines.push(Line::from("Generating draft".bold()));
            lines.push(Line::from(
                "The retained draft stays visible while a fresh response is requested.".dim(),
            ));
            lines.push(Line::from("Waiting for a validated JSON draft.".dim()));
            if let Some(draft) = generate.draft.as_ref() {
                lines.push(Line::from(""));
                lines.extend(render_draft_section(draft));
            }
            if let Some(prompt) = generate.prompt() {
                lines.push(Line::from(format!(
                    "prompt bytes: {}",
                    prompt.manifest.byte_count
                )));
            }
        }
        GeneratePhase::ContextReady => {
            if let Some(prompt) = generate.prompt() {
                lines.push(Line::from(""));
                match generate.prompt_view {
                    PromptView::Manifest => lines.extend(render_prompt_manifest(prompt)),
                    PromptView::Prompt => lines.extend(render_prompt_text(prompt)),
                }
            }
        }
        GeneratePhase::DraftReady => {
            lines.push(Line::from(""));
            lines.push(Line::from("Draft review".bold()));
            lines.push(Line::from(format!("status: {}", generate.review.summary)).cyan());
            lines.push(Line::from(
                "The generated draft is editable in the center pane.".dim(),
            ));
            if let Some(draft) = generate.draft.as_ref() {
                lines.push(Line::from(""));
                lines.extend(render_draft_section(draft));
            }
            if let Some(prompt) = generate.prompt() {
                lines.push(Line::from(""));
                lines.extend(render_manifest_warnings(prompt));
            }
            lines.push(Line::from(""));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "The execution preview will show branch, push, and tea commands before mutation."
                    .yellow(),
            ));
        }
        GeneratePhase::Failed => {
            lines.push(Line::from(""));
            lines.push(Line::from("Draft workflow failed".bold()));
            lines.push(Line::from(format!("status: {}", generate.review.summary)).cyan());
            if let Some(error) = &generate.context_error {
                lines.push(Line::from("Context failed".bold()));
                lines.push(Line::from(error.clone()).red());
            }
            if let Some(error) = &generate.generation_error {
                lines.push(Line::from("Generation failed".bold()));
                lines.push(Line::from(error.clone()).red());
            }
            if let Some(draft) = generate.draft.as_ref() {
                lines.push(Line::from(""));
                lines.extend(render_draft_section(draft));
            }
            if let Some(prompt) = generate.prompt() {
                lines.push(Line::from(""));
                lines.extend(render_manifest_warnings(prompt));
            }
            lines.push(Line::from(""));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press g to retry with the retained context.".dim(),
            ));
        }
        _ => {
            if let Some(draft) = generate.draft.as_ref() {
                lines.push(Line::from(""));
                lines.extend(render_draft_section(draft));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press Enter on the revset list to move to the PR form.".dim(),
            ));
            lines.push(Line::from(
                "Press g from navigation mode to generate using all form values.".dim(),
            ));
            lines.push(Line::from(
                "Press p to toggle prompt manifest and prompt text.".dim(),
            ));
        }
    }

    lines
}

fn render_draft_section(draft: &crate::generate::GeneratedDraft) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Generated draft".bold()),
        Line::from(format!("branch: {}", draft.branch_name).cyan()),
        Line::from(format!("title: {}", draft.title)),
        Line::from(format!("body chars: {}", draft.body.len())).dim(),
        Line::from(format!(
            "raw response chars: {}",
            draft.raw_model_response.len()
        ))
        .dim(),
        Line::from(""),
        Line::from("body".bold()),
    ];

    if draft.body.trim().is_empty() {
        lines.push(Line::from("  (empty)").dim());
    } else {
        for line in draft.body.lines() {
            lines.push(Line::from(format!("  {line}")));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "review notes: {}",
        draft.review_notes.len()
    )));
    if draft.review_notes.is_empty() {
        lines.push(Line::from("  (no review notes)").dim());
    } else {
        for note in &draft.review_notes {
            lines.push(Line::from(format!("  - {note}")));
        }
    }

    lines
}

fn render_manifest_warnings(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Prompt manifest warnings".bold())];

    if prompt.manifest.truncation_warnings.is_empty() {
        lines.push(Line::from("  (none)").dim());
    } else {
        for warning in &prompt.manifest.truncation_warnings {
            lines.push(Line::from(format!("  - {warning}")).yellow());
        }
    }

    lines
}

fn render_recent_logs(
    entries: &std::collections::VecDeque<String>,
    limit: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Recent logs".bold())];
    let recent: Vec<_> = entries.iter().rev().take(limit).cloned().collect();

    if recent.is_empty() {
        lines.push(Line::from("  (no logs yet)").dim());
    } else {
        for entry in recent.into_iter().rev() {
            lines.push(Line::from(format!("  {entry}")));
        }
    }

    lines
}
