use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, Screen};
use crate::generate::{FieldId, Focus, GeneratePhase, PromptView};
use crate::prompt::PromptBuild;

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
            status_line("jj", app.repo().jj.label()),
            status_line("git", app.repo().git.label()),
            status_line("tea", app.repo().tea.label()),
            status_line(
                "Workspace",
                if app.repo().inside_workspace {
                    "detected"
                } else {
                    "missing"
                },
            ),
            Line::from(match &app.repo().remote {
                Some(remote) => {
                    let warning = remote
                        .warning
                        .as_ref()
                        .map(|warning| format!(" ({warning})"))
                        .unwrap_or_default();
                    format!(
                        "Remote: {}@{}{}",
                        remote.display_name(),
                        remote.host,
                        warning
                    )
                }
                None => "Remote: pending".to_string(),
            }),
            Line::from(match &app.repo().remote {
                Some(remote) => format!("Remote URL: {}", remote.raw_url),
                None => "Remote URL: pending".to_string(),
            }),
            Line::from(format!("Base branch: {}", app.repo().base_branch.name)),
            Line::from(format!(
                "Ollama: {} {}",
                app.repo().ollama_base_url,
                app.repo().ollama_model
            )),
            Line::from(format!("Logs: {}", app.logs().entries.len())),
            Line::from(""),
            Line::from("Select a mode on the left.".dim()),
        ],
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

fn render_preview(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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

fn render_status(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let status = Line::from(vec![
        format!(" {} ", format!("{:?}", app.input_mode()).to_uppercase())
            .bold()
            .on_cyan(),
        format!(" {} ", app.screen().title()).dim(),
        format!(" phase:{:?} ", app.generate().phase).dim(),
        format!(" job:{:?} ", app.jobs().status()).dim(),
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
        " p ".bold().cyan(),
        "prompt ".dim(),
        " r ".bold().cyan(),
        "refresh ".dim(),
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

fn status_line(label: &'static str, value: &'static str) -> Line<'static> {
    match value {
        "available" | "detected" => Line::from(format!("{label}: {value}")),
        _ => Line::from(format!("{label}: {value}")).dim(),
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
    generate: &crate::generate::GenerateState,
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
        Line::from(format!("phase: {:?}", generate.phase).dim()),
        Line::from(format!("input mode: {:?}", app.input_mode()).dim()),
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
            if let Some(prompt) = generate.prompt_build() {
                lines.push(Line::from(format!(
                    "prompt bytes: {}",
                    prompt.manifest.byte_count
                )));
            }
        }
        GeneratePhase::ContextReady => {
            if let Some(prompt) = generate.prompt_build() {
                lines.push(Line::from(""));
                match generate.prompt_view {
                    PromptView::Manifest => lines.extend(render_prompt_manifest(&prompt)),
                    PromptView::Prompt => lines.extend(render_prompt_text(&prompt)),
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
            if let Some(prompt) = generate.prompt_build() {
                lines.push(Line::from(""));
                lines.extend(render_manifest_warnings(&prompt));
            }
            lines.push(Line::from(""));
            lines.extend(render_recent_logs(app.logs().entries.as_slice(), 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Execution preview only; branch, push, and tea mutation are not implemented yet."
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
            if let Some(prompt) = generate.prompt_build() {
                lines.push(Line::from(""));
                lines.extend(render_manifest_warnings(&prompt));
            }
            lines.push(Line::from(""));
            lines.extend(render_recent_logs(app.logs().entries.as_slice(), 6));
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

fn render_recent_logs(entries: &[String], limit: usize) -> Vec<Line<'static>> {
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
