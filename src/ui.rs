use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph, Wrap},
};

use crate::colors;

use crate::app::{App, JobRecord, Screen};
use crate::generate::{
    ExecutionPlan, FieldId, Focus, GeneratePhase, GenerateState, PromptView, RevsetSummary,
    StaleCheckResult,
};
use crate::prompt::PromptBuild;
use crate::repo::{LlmStatus, TeaAuth, ToolStatus};

pub fn render(frame: &mut Frame, app: &App) {
    let [main_area, status_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    if app.screen() == Screen::Landing {
        render_landing_hero(frame, app, main_area);
        render_status(frame, app, status_area);
        render_help(frame, app, help_area);
        return;
    }

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

fn render_landing_hero(frame: &mut Frame, app: &App, area: Rect) {
    let [header_area, actions_area, footer_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let header_center = center_horizontally(header_area);
    let actions_center = center_horizontally(actions_area);
    let footer_center = center_horizontally(footer_area);

    // Render header
    let header = Paragraph::new(vec![
        Line::from(""),
        Line::from("teatui".bold().fg(colors::TEXT)),
        Line::from("jj · Gitea · LLM".fg(colors::MUTED)),
        Line::from(""),
    ])
    .alignment(Alignment::Center);
    frame.render_widget(header, header_center);

    // Render actions list
    render_landing_actions(frame, app, actions_center);

    // Render footer with live tool status
    render_landing_footer(frame, app, footer_center);
}

/// Split a rect into 20%/60%/20% horizontal slices and return the centered column.
fn center_horizontally(area: Rect) -> Rect {
    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(60),
        Constraint::Percentage(20),
    ])
    .areas(area);
    center
}

fn render_landing_actions(frame: &mut Frame, app: &App, area: Rect) {
    let selected = app.landing().selected_entry;
    let center_width = area.width as usize;

    struct ActionItem {
        icon: &'static str,
        label: &'static str,
        key: &'static str,
    }

    let actions = [
        ActionItem {
            icon: "◆",
            label: "Generate PR",
            key: "g",
        },
        ActionItem {
            icon: "◆",
            label: "Manage PRs",
            key: "p",
        },
        ActionItem {
            icon: "◆",
            label: "Manage Issues",
            key: "i",
        },
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (index, action) in actions.iter().enumerate() {
        lines.push(landing_action_line(
            action.icon,
            action.label,
            action.key,
            index == selected,
            center_width,
        ));
        lines.push(Line::from("")); // blank spacer between items
    }

    // Quit hint — not a selectable entry, just shown as a static line.
    // Rendered with the same row builder so layout/padding logic stays in one place.
    lines.push(landing_action_line("◆", "Quit", "q", false, center_width));

    frame.render_widget(Paragraph::new(lines), area);
}

/// Build a single landing action row. The icon+label sits on the left and the
/// key hint is right-padded to the edge of `center_width`. Selected rows use a
/// `▶` prefix in ACCENT; unselected rows use two spaces with a MUTED icon and
/// TEXT label.
fn landing_action_line(
    icon: &'static str,
    label: &'static str,
    key: &'static str,
    is_selected: bool,
    center_width: usize,
) -> Line<'static> {
    let prefix = if is_selected { "▶ " } else { "  " };
    let left_len = prefix.chars().count() + icon.chars().count() + 1 + label.chars().count();
    let key_len = key.chars().count();
    let padding_len = center_width.saturating_sub(left_len + key_len).max(1);
    let padding = " ".repeat(padding_len);

    let spans = if is_selected {
        vec![
            Span::styled(
                format!("{prefix}{icon} {label}"),
                Style::new().bold().fg(colors::ACCENT),
            ),
            Span::raw(padding),
            Span::styled(key, Style::new().fg(colors::MUTED)),
        ]
    } else {
        vec![
            Span::styled(format!("{prefix}{icon} "), Style::new().fg(colors::MUTED)),
            Span::styled(label, Style::new().fg(colors::TEXT)),
            Span::raw(padding),
            Span::styled(key, Style::new().fg(colors::MUTED)),
        ]
    };
    Line::from(spans)
}

fn render_landing_footer(frame: &mut Frame, app: &App, area: Rect) {
    let repo = app.repo();
    let separator = || Span::raw("  ");
    let mut spans: Vec<Span> = Vec::new();

    let push_tool = |spans: &mut Vec<Span>, name: &'static str, status: &ToolStatus| {
        let (sym, style) = tool_status_indicator(status);
        spans.push(Span::styled(format!("{sym} {name}"), style));
    };

    push_tool(&mut spans, "jj", &repo.jj);
    spans.push(separator());
    push_tool(&mut spans, "git", &repo.git);
    spans.push(separator());
    push_tool(&mut spans, "tea", &repo.tea);
    spans.push(separator());

    // LLM backend
    if let Some(backend) = repo.llm_backends.iter().find(|b| b.name == repo.llm_active) {
        let (sym, style) = llm_status_indicator(&backend.status);
        spans.push(Span::styled(
            format!("{} LLM: {}/{}", sym, backend.name, backend.backend_type),
            style,
        ));
    } else {
        spans.push(Span::styled(
            "· LLM: (none)",
            Style::new().fg(colors::MUTED),
        ));
    }

    spans.push(separator());
    let (ws_sym, ws_style) = if repo.inside_workspace {
        ("✓", Style::new().fg(colors::GOOD))
    } else {
        ("·", Style::new().fg(colors::MUTED))
    };
    spans.push(Span::styled(format!("{ws_sym} workspace"), ws_style));

    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

fn tool_status_indicator(status: &ToolStatus) -> (&'static str, Style) {
    match status {
        ToolStatus::Available => ("✓", Style::new().fg(colors::GOOD)),
        ToolStatus::Missing | ToolStatus::Unknown => ("·", Style::new().fg(colors::MUTED)),
        ToolStatus::Error(_) => ("✗", Style::new().fg(colors::BAD)),
    }
}

fn llm_status_indicator(status: &LlmStatus) -> (&'static str, Style) {
    match status {
        LlmStatus::Reachable => ("✓", Style::new().fg(colors::GOOD)),
        LlmStatus::Unreachable(_) => ("✗", Style::new().fg(colors::BAD)),
        LlmStatus::Unknown(_) => ("·", Style::new().fg(colors::MUTED)),
    }
}

fn render_menu(frame: &mut Frame, app: &App, area: Rect) {
    match app.screen() {
        Screen::Generate => {
            render_generate_menu(frame, app, area);
        }
        _ => {
            let (items, title): (Vec<ListItem>, &'static str) = match app.screen() {
                Screen::Landing => (
                    selectable_list(
                        &["Generate PR", "Manage PRs", "Manage Issues"],
                        app.landing().selected_entry,
                    ),
                    "Modes",
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
                Screen::Generate => unreachable!(),
            };

            let list = List::new(items).block(themed_block(
                focused_title(title, app.focus() == Focus::Menu),
                app.focus() == Focus::Menu,
            ));

            frame.render_widget(list, area);
        }
    }
}

fn render_generate_menu(frame: &mut Frame, app: &App, area: Rect) {
    let block = themed_block(
        focused_title("Changes", app.focus() == Focus::Menu),
        app.focus() == Focus::Menu,
    );
    let inner = block.inner(area);
    // inner_width accounts for the horizontal padding of 1 on each side applied by themed_block
    let inner_width = inner.width.saturating_sub(2) as usize;

    let revsets = &app.generate().revsets;
    let selected_idx = app.generate().selected_revset;
    let last_idx = revsets.len().saturating_sub(1);

    let mut lines: Vec<Line<'static>> = Vec::new();

    for (index, revset) in revsets.iter().enumerate() {
        let is_selected = index == selected_idx;
        let row_lines = build_revset_row_lines(revset, is_selected, inner_width);
        lines.extend(row_lines);

        // Separator between rows (not after the last)
        if index < last_idx {
            let sep = "─".repeat(inner_width);
            lines.push(Line::from(sep).fg(colors::BORDER));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Build the display lines for one revset row in the per-change left column.
///
/// Priority for the primary identifier:
/// 1. First bookmark name (bold, ACCENT if selected)
/// 2. Description first line (if not a jj placeholder)
/// 3. Abbreviated change_id
///
/// Secondary line: if primary is a bookmark and description is meaningful,
/// show the description first line in muted style.
///
/// Text is wrapped to `inner_width` chars using char-based logic (no byte slicing).
fn build_revset_row_lines(
    revset: &RevsetSummary,
    is_selected: bool,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let first_bookmark = revset.bookmarks().first().map(|s| s.as_str()).unwrap_or("");
    let first_change_id = revset
        .change_ids()
        .first()
        .map(|s| s.as_str())
        .unwrap_or("");
    let desc = revset.description();

    let marker = if is_selected { "▶" } else { " " };
    let marker_width = 2; // "▶ " or "  " — one char + one space

    let available = inner_width.saturating_sub(marker_width);

    let (primary, secondary): (String, Option<String>) = if !first_bookmark.is_empty() {
        let sec = if !is_jj_default_description(desc) && !desc.is_empty() {
            Some(desc.to_string())
        } else {
            None
        };
        (first_bookmark.to_string(), sec)
    } else if !is_jj_default_description(desc) && !desc.is_empty() {
        (desc.to_string(), None)
    } else if !first_change_id.is_empty() {
        (first_change_id.to_string(), None)
    } else {
        (revset.label().to_string(), None)
    };

    let primary_is_bookmark = !first_bookmark.is_empty();

    // Wrap primary text into lines of `available` chars
    let primary_wrapped = wrap_chars(&primary, available);

    let mut result: Vec<Line<'static>> = Vec::new();

    for (i, wrapped_line) in primary_wrapped.iter().enumerate() {
        if i == 0 {
            // First line: marker + content
            if is_selected {
                let text = format!("{marker} {wrapped_line}");
                result.push(Line::from(text.bold().fg(colors::ACCENT)));
            } else if primary_is_bookmark {
                // Bookmark always bold even when not selected
                let spans = vec![
                    Span::styled(format!("{marker} "), Style::new().fg(colors::MUTED)),
                    Span::styled(wrapped_line.clone(), Style::new().bold().fg(colors::TEXT)),
                ];
                result.push(Line::from(spans));
            } else {
                result.push(Line::from(format!("{marker} {wrapped_line}")).fg(colors::MUTED));
            }
        } else {
            // Continuation lines: indented by marker_width spaces
            let indent = " ".repeat(marker_width);
            if is_selected {
                result.push(
                    Line::from(format!("{indent}{wrapped_line}"))
                        .style(Style::new().fg(colors::ACCENT)),
                );
            } else {
                result.push(Line::from(format!("{indent}{wrapped_line}")).fg(colors::MUTED));
            }
        }
    }

    // Optional secondary line (muted description when primary is a bookmark)
    if let Some(sec) = secondary {
        let indent = " ".repeat(marker_width);
        let truncated = truncate_chars(&sec, available);
        result.push(Line::from(format!("{indent}{truncated}")).fg(colors::MUTED));
    }

    result
}

/// Wrap `text` into lines of at most `max_chars` characters each.
/// Uses char-boundary-safe splitting. Never panics on multibyte chars.
fn wrap_chars(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![String::new()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + max_chars).min(chars.len());
        lines.push(chars[start..end].iter().collect());
        start = end;
    }
    lines
}

fn selectable_list(labels: &[&str], selected: usize) -> Vec<ListItem<'static>> {
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let marker = if index == selected { "▶" } else { " " };
            let line = format!("{marker} {label}");
            list_item(&line, index == selected)
        })
        .collect()
}

fn list_item(text: &str, selected: bool) -> ListItem<'static> {
    if selected {
        ListItem::new(text.to_string().bold().fg(colors::ACCENT))
    } else {
        ListItem::new(text.to_string().fg(colors::MUTED))
    }
}

fn is_jj_default_description(desc: &str) -> bool {
    let trimmed = desc.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("(no description set)")
}

/// Parse a raw jj `--stat` summary string into a compact human-readable form.
///
/// Handles the typical jj output shape:
/// `N files changed, X insertions(+), Y deletions(-)`
///
/// Returns e.g. `3 files · +42 / -7`.  Falls back to the first non-empty line
/// of `raw` when parsing fails.
pub fn compact_diff_stat(raw: &str) -> String {
    // jj --stat produces the summary line followed by per-file lines.
    // The last line of the block is the totals line (or it may be the only line).
    let summary_line = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .find(|l| l.contains("files changed") || l.contains("file changed"))
        .or_else(|| raw.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("")
        .trim();

    if summary_line.is_empty() {
        return String::new();
    }

    // Try to parse "N files changed, X insertions(+), Y deletions(-)"
    let files = parse_stat_count(summary_line, "file");
    if files.is_none() {
        // Fall back: return first non-empty line as-is.
        return summary_line.to_string();
    }
    let files = files.unwrap();
    let file_label = if files == 1 { "file" } else { "files" };

    let insertions = parse_stat_count(summary_line, "insertion").unwrap_or(0);
    let deletions = parse_stat_count(summary_line, "deletion").unwrap_or(0);

    if insertions == 0 && deletions == 0 {
        format!("{files} {file_label}")
    } else {
        format!("{files} {file_label} · +{insertions} / -{deletions}")
    }
}

/// Extract a count from a jj stat line for a given noun (e.g. "file", "insertion").
/// Matches patterns like "3 files changed" or "1 file changed".
fn parse_stat_count(line: &str, noun: &str) -> Option<u64> {
    // Walk the line tokens looking for `<number> <noun...>`.
    let lower = line.to_lowercase();
    let pos = lower.find(noun)?;
    // Walk backwards past whitespace to find the number token.
    let before = lower[..pos].trim_end();
    let num_str = before.split_whitespace().next_back()?;
    num_str.parse::<u64>().ok()
}

/// Render a two-line section header: a blank line followed by a bold title.
fn render_section_header(title: &str) -> Vec<Line<'static>> {
    vec![Line::from(""), Line::from(title.to_string().bold())]
}

/// Render the top-of-column identifier block for the selected change.
fn render_change_identifier(revset: &RevsetSummary) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // change_id in accent color (first change_id if multiple)
    let change_id = revset.change_ids().first().cloned().unwrap_or_default();
    lines.push(Line::from(change_id.fg(colors::ACCENT)));

    // Bookmarks (bold), omit line if empty
    if !revset.bookmarks().is_empty() {
        let bookmark_spans: Vec<ratatui::text::Span<'static>> = revset
            .bookmarks()
            .iter()
            .enumerate()
            .flat_map(|(i, b)| {
                let mut spans = vec![ratatui::text::Span::styled(
                    b.clone(),
                    ratatui::style::Style::new().bold(),
                )];
                if i + 1 < revset.bookmarks().len() {
                    spans.push(ratatui::text::Span::raw(", "));
                }
                spans
            })
            .collect();
        lines.push(Line::from(bookmark_spans));
    }

    // Description subject line
    let desc = revset.description();
    if !is_jj_default_description(desc) {
        lines.push(Line::from(desc.to_string()));
    }

    // Description body (indented), omit if empty/placeholder
    if revset.is_meaningful_body() {
        for body_line in revset.description_body().lines() {
            lines.push(Line::from(format!("  {body_line}")));
        }
    }

    // Compact diff stat
    let stat = compact_diff_stat(revset.stats());
    if !stat.is_empty() {
        lines.push(Line::from(stat.fg(colors::MUTED)));
    }

    // Scope revset (muted, for power users)
    lines.push(Line::from(revset.label().to_string().fg(colors::MUTED)));

    lines
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    s[..end].to_string()
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
                lines.push(Line::from(format!("Remote URL: {}", remote.raw_url)).fg(colors::MUTED));
                if let Some(warning) = &remote.warning {
                    lines.push(Line::from(format!("Remote warning: {warning}")).fg(colors::WARN));
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
                    TeaAuth::Error(_) => Line::from(detail.to_string()).fg(colors::BAD),
                    _ => Line::from(detail.to_string()).fg(colors::MUTED),
                });
            }
            if let TeaAuth::Configured { host, user } = &repo.tea_auth {
                lines.push(Line::from(format!("Tea host: {host}")).fg(colors::GOOD));
                if let Some(user) = user {
                    lines.push(Line::from(format!("Tea user: {user}")).fg(colors::GOOD));
                }
            }

            lines.extend(render_llm_lines(repo));

            lines.push(Line::from(format!(
                "Base branch: {}",
                repo.base_branch.name
            )));
            lines.push(Line::from(format!("Logs: {}", app.logs().entries.len())).fg(colors::MUTED));
            lines.push(Line::from(""));
            lines.push(Line::from("Select a mode on the left.".fg(colors::MUTED)));
            lines
        }
        Screen::Generate
            if app.input_mode() == crate::generate::InputMode::Editing
                && app.focus() == Focus::Form =>
        {
            render_generate_editor(frame, app, area);
            return;
        }
        Screen::Generate => render_generate_fields(app, area),
        Screen::PullRequests => vec![
            Line::from("Manage PRs".bold()),
            Line::from(""),
            Line::from(
                "List open PRs, preview details, and add a simple comment.".fg(colors::MUTED),
            ),
            Line::from("This mode stays intentionally small.".fg(colors::MUTED)),
        ],
        Screen::Issues => vec![
            Line::from("Manage Issues".bold()),
            Line::from(""),
            Line::from(
                "List open issues, preview details, and add a simple comment.".fg(colors::MUTED),
            ),
            Line::from("This mode stays intentionally small.".fg(colors::MUTED)),
        ],
    };

    let title = match app.screen() {
        Screen::Landing => "Status",
        Screen::Generate => generate_work_title(app.generate().phase),
        Screen::PullRequests | Screen::Issues => "Work",
    };

    let form = Paragraph::new(lines)
        .block(themed_block(
            focused_title(title, app.focus() == Focus::Form),
            app.focus() == Focus::Form,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(form, area);
}

fn render_generate_fields(app: &App, area: Rect) -> Vec<Line<'static>> {
    let total = FieldId::ALL.len();
    let last = total.saturating_sub(1);
    let sep_width = area.width.saturating_sub(6) as usize;
    let separator = format!("  {}  ", "─".repeat(sep_width));
    FieldId::ALL
        .iter()
        .enumerate()
        .flat_map(|(index, field_id)| {
            let mut lines = render_generate_field(
                app.generate(),
                *field_id,
                index == app.generate().selected_field,
                index == app.generate().selected_field && app.focus() == Focus::Form,
                total,
            );
            if index < last {
                lines.push(Line::from(separator.clone()).fg(colors::BORDER));
            }
            lines
        })
        .collect()
}

fn render_generate_editor(frame: &mut Frame, app: &App, area: Rect) {
    let title = generate_work_title(app.generate().phase);
    let block = themed_block(focused_title(title, true), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let selected = app.generate().selected_field();
    let total = FieldId::ALL.len();
    let before = FieldId::ALL
        .iter()
        .take_while(|field_id| **field_id != selected)
        .enumerate()
        .flat_map(|(index, field_id)| {
            render_generate_field(
                app.generate(),
                *field_id,
                index == app.generate().selected_field,
                false,
                total,
            )
        })
        .collect::<Vec<_>>();
    let after = FieldId::ALL
        .iter()
        .skip_while(|field_id| **field_id != selected)
        .skip(1)
        .enumerate()
        .flat_map(|(offset, field_id)| {
            let index = app.generate().selected_field + offset + 1;
            render_generate_field(
                app.generate(),
                *field_id,
                index == app.generate().selected_field,
                false,
                total,
            )
        })
        .collect::<Vec<_>>();

    let field = app.generate().form.field(selected);
    let editor_height = if matches!(selected, FieldId::Description) {
        6
    } else {
        1
    };
    let [
        before_area,
        header_area,
        editor_area,
        errors_area,
        after_area,
    ] = Layout::vertical([
        Constraint::Length(before.len() as u16),
        Constraint::Length(1),
        Constraint::Length(editor_height),
        Constraint::Length(field.errors.len() as u16),
        Constraint::Fill(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(before).wrap(Wrap { trim: false }),
        before_area,
    );
    let editor_header = format!(
        "▶ {}  ({}/{})",
        selected.label(),
        app.generate().selected_field + 1,
        total
    );
    frame.render_widget(
        Paragraph::new(Line::from(editor_header).style(Style::new().bold().fg(colors::ACCENT))),
        header_area,
    );
    frame.render_widget(&field.editor, editor_area);

    let errors = field
        .errors
        .iter()
        .map(|error| Line::from(format!("    - {error}")).fg(colors::BAD))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(errors), errors_area);
    frame.render_widget(Paragraph::new(after).wrap(Wrap { trim: false }), after_area);
}

fn generate_work_title(phase: GeneratePhase) -> &'static str {
    if phase == GeneratePhase::Confirming {
        "Execution Preview"
    } else if phase == GeneratePhase::CheckingFreshness {
        "Verifying Repo Context"
    } else if phase == GeneratePhase::DraftReady {
        "Draft Review"
    } else {
        "PR Form"
    }
}

fn render_preview(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.screen() {
        Screen::Landing => {
            let mut lines = vec![
                Line::from("Landing".bold()),
                Line::from(""),
                Line::from("Generate PR, Manage PRs, and Manage Issues are separate modes."),
                Line::from("Press Enter to open the selected mode.".fg(colors::MUTED)),
            ];

            let blockers = app.repo().blocker_lines();
            if !blockers.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from("Setup blockers".bold()));
                for blocker in blockers {
                    lines.push(Line::from(format!("- {blocker}")).fg(colors::BAD));
                }
            }

            lines
        }
        Screen::Generate => render_generate_preview(app),
        Screen::PullRequests => vec![
            Line::from("PR Preview".bold()),
            Line::from(""),
            Line::from("Selected PR body, status, and comments will appear here."),
            Line::from("Esc returns to Landing.".fg(colors::MUTED)),
        ],
        Screen::Issues => vec![
            Line::from("Issue Preview".bold()),
            Line::from(""),
            Line::from("Selected issue body and comments will appear here."),
            Line::from("Esc returns to Landing.".fg(colors::MUTED)),
        ],
    };

    let preview = Paragraph::new(lines)
        .block(themed_block(
            focused_title("Preview", app.focus() == Focus::Preview),
            app.focus() == Focus::Preview,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let focus = match app.focus() {
        Focus::Menu => "focus:menu",
        Focus::Form => "focus:form",
        Focus::Preview => "focus:preview",
    };

    let mut raw_segments: Vec<Span<'static>> = vec![
        format!(" {} ", app.input_mode().label())
            .bold()
            .fg(colors::BASE)
            .bg(colors::ACCENT),
        format!(" {} ", app.screen().title()).fg(colors::MUTED),
        format!(" {focus} ").fg(colors::MUTED),
    ];

    if app.screen() == Screen::Generate {
        raw_segments.push(format!(" phase:{} ", app.generate().phase.label()).fg(colors::MUTED));
        let job_segment = app
            .jobs()
            .active_status()
            .map(|status| format!(" job:{} ", status.label()))
            .unwrap_or_else(|| " job:idle ".to_string());
        raw_segments.push(job_segment.fg(colors::MUTED));
        let prompt_mode = match app.generate().prompt_view {
            PromptView::Manifest => "prompt:manifest",
            PromptView::Prompt => "prompt:text",
        };
        raw_segments.push(format!(" {prompt_mode} ").fg(colors::MUTED));
    }

    let divider = Span::styled(" │ ", Style::new().fg(colors::SURFACE1));
    let mut segments: Vec<Span<'static>> = Vec::with_capacity(raw_segments.len() * 2);
    let last_idx = raw_segments.len().saturating_sub(1);
    for (i, seg) in raw_segments.into_iter().enumerate() {
        segments.push(seg);
        if i < last_idx {
            segments.push(divider.clone());
        }
    }

    frame.render_widget(Paragraph::new(Line::from(segments)), area);
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.screen() {
        Screen::Landing => Line::from(vec![
            " Enter ".bold().fg(colors::ACCENT),
            "open ".fg(colors::MUTED),
            " q ".bold().fg(colors::ACCENT),
            "quit ".fg(colors::MUTED),
        ]),
        Screen::Generate if app.input_mode() == crate::generate::InputMode::Editing => {
            Line::from(vec![
                " typing ".bold().fg(colors::ACCENT),
                "edit field ".fg(colors::MUTED),
                " Enter ".bold().fg(colors::ACCENT),
                "save single-line / newline description ".fg(colors::MUTED),
                " Ctrl+S ".bold().fg(colors::ACCENT),
                "commit description ".fg(colors::MUTED),
                " Esc ".bold().fg(colors::ACCENT),
                "cancel ".fg(colors::MUTED),
            ])
        }
        Screen::Generate if app.input_mode() == crate::generate::InputMode::Confirm => {
            Line::from(vec![
                " Enter ".bold().fg(colors::ACCENT),
                "execute ".fg(colors::MUTED),
                " Esc ".bold().fg(colors::ACCENT),
                "cancel ".fg(colors::MUTED),
            ])
        }
        Screen::Generate if app.generate().phase == GeneratePhase::CheckingFreshness => {
            Line::from(vec![
                " Esc ".bold().fg(colors::ACCENT),
                "cancel ".fg(colors::MUTED),
                " waiting ".fg(colors::MUTED),
                "verifying repo context ".fg(colors::MUTED),
            ])
        }
        Screen::Generate if app.generate().phase == GeneratePhase::Executing => Line::from(vec![
            " waiting ".fg(colors::MUTED),
            "execution in progress ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "ignored ".fg(colors::MUTED),
        ]),
        Screen::Generate if app.generate().phase == GeneratePhase::Complete => Line::from(vec![
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
            " execution done ".fg(colors::MUTED),
        ]),
        Screen::Generate if app.generate().phase == GeneratePhase::Failed => Line::from(vec![
            " c ".bold().fg(colors::ACCENT),
            "retry ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Screen::Generate if app.focus() == Focus::Preview => Line::from(vec![
            " p ".bold().fg(colors::ACCENT),
            "toggle prompt ".fg(colors::MUTED),
            " g ".bold().fg(colors::ACCENT),
            "regenerate ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Screen::Generate => Line::from(vec![
            " h/l ".bold().fg(colors::ACCENT),
            "move focus ".fg(colors::MUTED),
            " Enter ".bold().fg(colors::ACCENT),
            "select/edit ".fg(colors::MUTED),
            " i ".bold().fg(colors::ACCENT),
            "edit ".fg(colors::MUTED),
            " g ".bold().fg(colors::ACCENT),
            "generate ".fg(colors::MUTED),
            " c ".bold().fg(colors::ACCENT),
            "confirm ".fg(colors::MUTED),
            " p ".bold().fg(colors::ACCENT),
            "prompt ".fg(colors::MUTED),
            " r ".bold().fg(colors::ACCENT),
            "refresh ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Screen::PullRequests | Screen::Issues => Line::from(vec![
            " Enter ".bold().fg(colors::ACCENT),
            "select ".fg(colors::MUTED),
            " c ".bold().fg(colors::ACCENT),
            "comment ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
    };
    frame.render_widget(Paragraph::new(help), area);
}

fn focused_title(title: &'static str, focused: bool) -> Line<'static> {
    if focused {
        Line::from(title.bold().fg(colors::ACCENT))
    } else {
        Line::from(title.fg(colors::MUTED))
    }
}

fn themed_block(title: Line<'static>, focused: bool) -> Block<'static> {
    let border_style = if focused {
        Style::new().fg(colors::FOCUSED_BORDER)
    } else {
        Style::new().fg(colors::BORDER)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .title(title)
        .padding(Padding::horizontal(1))
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
        StatusTone::Good => line.fg(colors::GOOD),
        StatusTone::Muted => line.fg(colors::MUTED),
        StatusTone::Bad => line.fg(colors::BAD),
    }
}

fn render_llm_lines(repo: &crate::repo::RepoState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let active_backend = repo
        .llm_backends
        .iter()
        .find(|backend| backend.name == repo.llm_active);

    if let Some(backend) = active_backend {
        lines.push(status_line(
            "LLM",
            format!("{} ({})", backend.name, backend.backend_type),
            match &backend.status {
                LlmStatus::Reachable => StatusTone::Good,
                LlmStatus::Unreachable(_) => StatusTone::Bad,
                LlmStatus::Unknown(_) => StatusTone::Muted,
            },
        ));
        lines.push(Line::from(format!("LLM endpoint: {}", backend.base_url)).fg(colors::MUTED));
        lines.push(Line::from(format!("LLM model: {}", backend.model)).fg(colors::MUTED));
        if let Some(detail) = backend.status.detail() {
            lines.push(match &backend.status {
                LlmStatus::Unreachable(_) => Line::from(detail.to_string()).fg(colors::BAD),
                _ => Line::from(detail.to_string()).fg(colors::MUTED),
            });
        }
    } else {
        lines.push(status_line("LLM", "(no active backend)", StatusTone::Muted));
    }

    if repo.llm_backends.len() > 1 {
        lines.push(Line::from("LLM backends".bold()));
        for backend in &repo.llm_backends {
            let marker = if backend.name == repo.llm_active {
                "*"
            } else {
                " "
            };
            let tone = match &backend.status {
                LlmStatus::Reachable => StatusTone::Good,
                LlmStatus::Unreachable(_) => StatusTone::Bad,
                LlmStatus::Unknown(_) => StatusTone::Muted,
            };
            let label = format!("{marker} {}", backend.name);
            lines.push(status_line(
                &label,
                format!("{} {}", backend.backend_type, backend.status.label()),
                tone,
            ));
        }
    }

    lines
}

fn render_prompt_manifest(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let manifest = &prompt.manifest;
    let mut lines = vec![
        Line::from("Prompt manifest".bold()),
        Line::from(""),
        Line::from(format!("selected revset: {}", manifest.selected_revset).fg(colors::ACCENT)),
        Line::from(format!("base branch: {}", manifest.base_branch)),
        Line::from(format!("prompt bytes: {}", manifest.byte_count)).fg(colors::MUTED),
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
            lines.push(Line::from(warning.clone()).fg(colors::WARN));
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
                .fg(colors::BAD),
            );
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Press p to view the full prompt text.".fg(colors::MUTED),
    ));
    lines
}

fn render_prompt_text(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Prompt text".bold()), Line::from("")];

    for line in prompt.prompt.lines() {
        lines.push(Line::from(line.to_string()));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Press p to return to the manifest.".fg(colors::MUTED),
    ));
    lines
}

fn render_generate_field(
    generate: &GenerateState,
    field_id: FieldId,
    selected: bool,
    focused: bool,
    total_fields: usize,
) -> Vec<Line<'static>> {
    let field = generate.form.field(field_id);
    let label = field_id.label();
    let value = field.display_value().to_string();
    let error_count = field.errors.len();
    let marker = if selected { "▶" } else { " " };
    let index_suffix = if focused {
        let n = generate.selected_field + 1;
        format!("  ({n}/{})", total_fields)
    } else {
        String::new()
    };
    let header = if matches!(field_id, FieldId::Description) {
        if error_count > 0 {
            format!("{marker} {label} ({error_count} errors){index_suffix}")
        } else {
            format!("{marker} {label}{index_suffix}")
        }
    } else if error_count > 0 {
        format!("{marker} {label}: {value} ({error_count} errors){index_suffix}")
    } else {
        format!("{marker} {label}: {value}{index_suffix}")
    };

    let mut lines =
        Vec::with_capacity(1 + error_count + usize::from(matches!(field_id, FieldId::Description)));
    if focused {
        lines.push(Line::from(header.bold().fg(colors::ACCENT)));
    } else {
        lines.push(Line::from(header.fg(colors::MUTED)));
    }

    if matches!(field_id, FieldId::Description) {
        if value.trim().is_empty() {
            lines.push(Line::from("    (empty)").fg(colors::MUTED));
        } else {
            for line in value.lines() {
                lines.push(Line::from(format!("    {line}")));
            }
        }
    }

    for error in &field.errors {
        lines.push(Line::from(format!("    - {error}")).fg(colors::BAD));
    }

    lines
}

fn render_generate_preview(app: &App) -> Vec<Line<'static>> {
    let generate = app.generate();
    let revset = generate.selected_revset();

    // Always-visible identifier block at the top.
    let mut lines = render_change_identifier(revset);

    // Base branch as a muted footer line of the identifier block.
    lines.push(Line::from(
        format!("base: {}", app.repo().base_branch.name,).fg(colors::MUTED),
    ));

    // Phase-specific section.
    let phase_title = generate_work_title(generate.phase);
    lines.extend(render_section_header(phase_title));

    match generate.phase {
        GeneratePhase::CollectingContext => {
            // Status
            lines.push(Line::from("Collecting context…".fg(colors::ACCENT)));
            lines.push(Line::from(
                format!("base branch: {}", generate.form.base.display_value()).fg(colors::MUTED),
            ));
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
        }
        GeneratePhase::Generating => {
            // Status
            lines.push(Line::from("Generating draft…".fg(colors::ACCENT)));
            lines.push(Line::from(
                "Waiting for a validated JSON draft.".fg(colors::MUTED),
            ));
            // Details
            if let Some(draft) = generate.draft.as_ref() {
                lines.extend(render_section_header("Draft"));
                lines.extend(render_draft_section(draft));
            }
            if let Some(prompt) = generate.prompt() {
                lines.push(Line::from(
                    format!("prompt bytes: {}", prompt.manifest.byte_count).fg(colors::MUTED),
                ));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
        }
        GeneratePhase::ContextReady => {
            // Status
            lines.push(Line::from("Context ready.".fg(colors::GOOD)));
            // Details: prompt manifest or prompt text
            if let Some(prompt) = generate.prompt() {
                lines.extend(render_section_header("Prompt"));
                match generate.prompt_view {
                    PromptView::Manifest => lines.extend(render_prompt_manifest(prompt)),
                    PromptView::Prompt => lines.extend(render_prompt_text(prompt)),
                }
            }
        }
        GeneratePhase::DraftReady => {
            // Status
            lines.push(
                Line::from(format!("status: {}", generate.review.summary)).fg(colors::ACCENT),
            );
            lines.push(Line::from(
                "The generated draft is editable in the center pane.".fg(colors::MUTED),
            ));
            // Details: draft + manifest warnings
            if let Some(draft) = generate.draft.as_ref() {
                lines.extend(render_section_header("Draft"));
                lines.extend(render_draft_section(draft));
            }
            if let Some(prompt) = generate.prompt() {
                lines.extend(render_section_header("Manifest warnings"));
                lines.extend(render_manifest_warnings(prompt));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "The execution preview will show branch, push, and tea commands before mutation."
                    .fg(colors::WARN),
            ));
            lines.push(Line::from(
                "Press c to validate the execution plan and check repo freshness."
                    .fg(colors::MUTED),
            ));
        }
        GeneratePhase::CheckingFreshness => {
            // Status
            lines.push(Line::from(
                generate
                    .confirmation_summary
                    .as_deref()
                    .map(|s| format!("validation: {s}"))
                    .unwrap_or_else(|| "validation: running".to_string()),
            ));
            lines.push(Line::from("freshness: verifying repo context…").fg(colors::WARN));
            // Details: draft
            if let Some(draft) = generate.draft.as_ref() {
                lines.extend(render_section_header("Draft"));
                lines.extend(render_draft_section(draft));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Wait for the freshness check to finish.".fg(colors::MUTED),
            ));
        }
        GeneratePhase::Confirming => {
            // Status
            lines.push(Line::from(
                generate
                    .confirmation_summary
                    .as_deref()
                    .map(|s| format!("validation: {s}"))
                    .unwrap_or_else(|| "validation: passed".to_string()),
            ));
            lines.push(match generate.freshness_result.as_ref() {
                Some(StaleCheckResult::Fresh) => Line::from("freshness: verified").fg(colors::GOOD),
                Some(StaleCheckResult::Stale { reason }) => {
                    Line::from(format!("freshness: stale - {reason}")).fg(colors::BAD)
                }
                None => Line::from("freshness: unavailable").fg(colors::WARN),
            });
            // Details: execution plan
            if let Some(plan) = generate.execution_plan.as_ref() {
                lines.extend(render_section_header("Execution plan"));
                lines.extend(render_execution_plan(plan));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press Enter to start execution.".fg(colors::WARN),
            ));
        }
        GeneratePhase::Executing => {
            // Status
            if let Some(step) = generate.execution_step {
                let total = generate.execution_total.unwrap_or(0);
                lines.push(Line::from(format!("step: {}/{}", step + 1, total)).fg(colors::ACCENT));
            } else {
                lines.push(Line::from("Executing…".fg(colors::ACCENT)));
            }
            lines.push(Line::from(
                "Wait for the current command to finish.".fg(colors::MUTED),
            ));
            // Details: job registry + execution plan
            lines.extend(render_section_header("Jobs"));
            lines.extend(render_job_records(&app.jobs().records));
            if let Some(plan) = generate.execution_plan.as_ref() {
                lines.extend(render_section_header("Execution plan"));
                lines.extend(render_execution_plan(plan));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
        }
        GeneratePhase::Complete => {
            // Status
            lines.push(Line::from("Execution complete.".fg(colors::GOOD)));
            if let Some(completion) = generate.completion.as_ref() {
                lines.push(Line::from(match completion.pr_url.as_ref() {
                    Some(url) => format!("PR URL: {url}"),
                    None => "PR URL: (not parsed)".to_string(),
                }));
                // Details: execution plan
                lines.extend(render_section_header("Execution plan"));
                lines.extend(render_execution_plan(&completion.plan));
            } else {
                lines.push(Line::from("completion details unavailable").fg(colors::BAD));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press Esc to return to the draft review.".fg(colors::MUTED),
            ));
        }
        GeneratePhase::Failed => {
            // Status
            lines.push(
                Line::from(format!("status: {}", generate.review.summary)).fg(colors::ACCENT),
            );
            if let Some(error) = &generate.context_error {
                lines.push(Line::from("Context collection failed:".bold()));
                lines.push(Line::from(error.clone()).fg(colors::BAD));
            }
            if let Some(error) = &generate.generation_error {
                lines.push(Line::from("Generation failed:".bold()));
                lines.push(Line::from(error.clone()).fg(colors::BAD));
            }
            if let Some(summary) = generate.confirmation_summary.as_ref() {
                lines.push(Line::from(format!("validation: {summary}")).fg(colors::ACCENT));
            }
            if let Some(result) = generate.freshness_result.as_ref() {
                lines.push(match result {
                    StaleCheckResult::Fresh => Line::from("freshness: verified").fg(colors::GOOD),
                    StaleCheckResult::Stale { reason } => {
                        Line::from(format!("freshness: stale - {reason}")).fg(colors::BAD)
                    }
                });
            }
            if let Some(step) = generate.execution_failed_step {
                lines.push(
                    Line::from(format!("execution failed at step {}", step + 1)).fg(colors::BAD),
                );
            }
            if let Some(error) = &generate.execution_error {
                lines.push(Line::from(error.clone()).fg(colors::BAD));
            }
            // Details
            if let Some(draft) = generate.draft.as_ref() {
                lines.extend(render_section_header("Draft"));
                lines.extend(render_draft_section(draft));
            }
            if let Some(prompt) = generate.prompt() {
                lines.extend(render_section_header("Manifest warnings"));
                lines.extend(render_manifest_warnings(prompt));
            }
            if let Some(plan) = generate.execution_plan.as_ref() {
                lines.extend(render_section_header("Execution plan"));
                lines.extend(render_execution_plan(plan));
            }
            // Recent logs
            lines.extend(render_section_header("Logs"));
            lines.extend(render_recent_logs(&app.logs().entries, 6));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press c to retry with the retained context.".fg(colors::MUTED),
            ));
        }
        _ => {
            // SelectingRevset / EditingForm
            if let Some(draft) = generate.draft.as_ref() {
                lines.extend(render_section_header("Draft"));
                lines.extend(render_draft_section(draft));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Press Enter on the revset list to move to the PR form.".fg(colors::MUTED),
            ));
            lines.push(Line::from(
                "Press g from navigation mode to generate using all form values.".fg(colors::MUTED),
            ));
            lines.push(Line::from(
                "Press p to toggle prompt manifest and prompt text.".fg(colors::MUTED),
            ));
        }
    }

    lines
}

fn render_draft_section(draft: &crate::generate::GeneratedDraft) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("branch: {}", draft.branch_name).fg(colors::ACCENT)),
        Line::from(format!("title: {}", draft.title)),
        Line::from(format!("body chars: {}", draft.body.len())).fg(colors::MUTED),
        Line::from(format!(
            "raw response chars: {}",
            draft.raw_model_response.len()
        ))
        .fg(colors::MUTED),
        Line::from(""),
        Line::from("body".bold()),
    ];

    if draft.body.trim().is_empty() {
        lines.push(Line::from("  (empty)").fg(colors::MUTED));
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
        lines.push(Line::from("  (no review notes)").fg(colors::MUTED));
    } else {
        for note in &draft.review_notes {
            lines.push(Line::from(format!("  - {note}")));
        }
    }

    lines
}

fn render_execution_plan(plan: &ExecutionPlan) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for (index, step) in plan.steps.iter().enumerate() {
        lines.push(Line::from(format!("{}. {}", index + 1, step.label)).fg(colors::ACCENT));
        lines.push(Line::from(format!("   {}", step.command.redacted_display())).fg(colors::MUTED));
    }

    lines
}

fn render_job_records(records: &[JobRecord]) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Job registry".bold())];

    if records.is_empty() {
        lines.push(Line::from("  (no jobs yet)").fg(colors::MUTED));
        return lines;
    }

    for record in records {
        let marker = match record.status {
            crate::event::JobStatus::Queued => "[queued]",
            crate::event::JobStatus::Running => "[running]",
            crate::event::JobStatus::Succeeded => "[succeeded]",
            crate::event::JobStatus::Failed => "[failed]",
            crate::event::JobStatus::TimedOut => "[timed-out]",
        };
        lines.push(Line::from(format!(
            "{} {} {}",
            record.name, marker, record.command
        )));
        if let Some(duration) = record.duration {
            lines.push(Line::from(format!("   duration: {:?}", duration)).fg(colors::MUTED));
        }
        if record.status.is_active() {
            lines.push(Line::from("   still running".fg(colors::MUTED)));
        }
        if !record.stderr.trim().is_empty() {
            lines.push(Line::from(format!("   stderr: {}", record.stderr.trim())).fg(colors::BAD));
        }
        if !record.stdout.trim().is_empty()
            && !matches!(record.status, crate::event::JobStatus::Succeeded)
        {
            lines
                .push(Line::from(format!("   stdout: {}", record.stdout.trim())).fg(colors::MUTED));
        }
    }

    lines
}

fn render_manifest_warnings(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if prompt.manifest.truncation_warnings.is_empty() {
        lines.push(Line::from("  (none)").fg(colors::MUTED));
    } else {
        for warning in &prompt.manifest.truncation_warnings {
            lines.push(Line::from(format!("  - {warning}")).fg(colors::WARN));
        }
    }

    lines
}

fn render_recent_logs(
    entries: &std::collections::VecDeque<String>,
    limit: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let recent: Vec<_> = entries.iter().rev().take(limit).cloned().collect();

    if recent.is_empty() {
        lines.push(Line::from("  (no logs yet)").fg(colors::MUTED));
    } else {
        for entry in recent.into_iter().rev() {
            lines.push(Line::from(format!("  {entry}")));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::{compact_diff_stat, is_jj_default_description, truncate_chars, wrap_chars};

    #[test]
    fn jj_default_description_recognises_placeholder() {
        assert!(is_jj_default_description(""));
        assert!(is_jj_default_description("   "));
        assert!(is_jj_default_description("(no description set)"));
        assert!(is_jj_default_description("  (No Description Set)  "));
        assert!(!is_jj_default_description("feat: add config loader"));
    }

    #[test]
    fn truncate_chars_handles_ascii_and_multibyte() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello", 0), "");
        // Multi-byte: each emoji is multiple bytes; must not panic on byte index.
        assert_eq!(truncate_chars("héllo", 4), "héll");
        assert_eq!(truncate_chars("🚀🚀🚀", 2), "🚀🚀");
    }

    #[test]
    fn wrap_chars_splits_on_char_boundaries() {
        assert_eq!(wrap_chars("hello world", 5), vec!["hello", " worl", "d"]);
        assert_eq!(wrap_chars("short", 10), vec!["short"]);
        // Empty input still yields one line so callers can emit the marker row.
        assert_eq!(wrap_chars("", 5), vec![""]);
        // Zero width must not panic and produces a single empty line.
        assert_eq!(wrap_chars("abc", 0), vec![""]);
        // Multi-byte chars must split on char boundaries, not byte boundaries.
        assert_eq!(wrap_chars("héllo wörld", 5), vec!["héllo", " wörl", "d"]);
        assert_eq!(wrap_chars("🚀🚀🚀🚀", 2), vec!["🚀🚀", "🚀🚀"]);
    }

    #[test]
    fn compact_diff_stat_typical_jj_output() {
        assert_eq!(
            compact_diff_stat("3 files changed, 42 insertions(+), 7 deletions(-)"),
            "3 files · +42 / -7"
        );
    }

    #[test]
    fn compact_diff_stat_one_file() {
        assert_eq!(
            compact_diff_stat("1 file changed, 5 insertions(+), 0 deletions(-)"),
            "1 file · +5 / -0"
        );
    }

    #[test]
    fn compact_diff_stat_insertions_only() {
        assert_eq!(
            compact_diff_stat("2 files changed, 10 insertions(+)"),
            "2 files · +10 / -0"
        );
    }

    #[test]
    fn compact_diff_stat_zero_files() {
        // jj may emit "0 files changed, ..." for empty diffs.
        assert_eq!(
            compact_diff_stat("0 files changed, 0 insertions(+), 0 deletions(-)"),
            "0 files"
        );
    }

    #[test]
    fn compact_diff_stat_empty_input() {
        assert_eq!(compact_diff_stat(""), "");
    }

    #[test]
    fn compact_diff_stat_multiline_with_summary_last() {
        // jj --stat prints per-file lines followed by the summary line.
        let raw = "src/foo.rs | 10 ++++------\nsrc/bar.rs | 5 ++---\n2 files changed, 7 insertions(+), 8 deletions(-)";
        assert_eq!(compact_diff_stat(raw), "2 files · +7 / -8");
    }

    #[test]
    fn compact_diff_stat_fallback_on_unrecognised_format() {
        // When the input doesn't contain "files changed" the first non-empty
        // line is returned verbatim.
        let raw = "some weird stat output";
        assert_eq!(compact_diff_stat(raw), "some weird stat output");
    }
}
