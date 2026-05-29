use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
};

use crate::colors;

use crate::app::{App, JobRecord, PrCommentPhase, Screen};
use crate::generate::{
    ExecutionPlan, FieldId, Focus, GeneratePhase, GenerateState, InputMode, PickerOptionView,
    PromptView, RevsetSummary, StaleCheckResult,
};
use crate::prompt::PromptBuild;
use crate::pull_requests::PullRequestSummary;
use crate::repo::{LlmStatus, TeaAuth, ToolStatus};

const DESCRIPTION_FIELD_DISPLAY_LINES: usize = 6;

pub fn render(frame: &mut Frame, app: &mut App) {
    let [main_area, status_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    if app.screen() == Screen::Landing {
        render_landing_hero(frame, &*app, main_area);
        render_status(frame, &*app, status_area);
        render_help(frame, &*app, help_area);
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
    render_status(frame, &*app, status_area);
    render_help(frame, &*app, help_area);

    // Render picker modal last so it overlays all other panes.
    render_picker_modal(frame, app, frame.area());

    // Render PR comment modal on top of everything when active.
    render_pr_comment_modal(frame, app, frame.area());
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
    let (ws_sym, ws_label, ws_style) = if repo.discovering {
        (
            "·",
            "discovering workspace…",
            Style::new().fg(colors::MUTED),
        )
    } else if repo.inside_workspace {
        ("✓", "workspace", Style::new().fg(colors::GOOD))
    } else {
        ("·", "no jj workspace", Style::new().fg(colors::MUTED))
    };
    spans.push(Span::styled(format!("{ws_sym} {ws_label}"), ws_style));

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

fn render_menu(frame: &mut Frame, app: &mut App, area: Rect) {
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
                Screen::PullRequests => render_pull_request_menu(app),
                Screen::Issues => (
                    selectable_list(
                        &["Open issues", "Filter", "Details"],
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

fn render_pull_request_menu(app: &App) -> (Vec<ListItem<'static>>, &'static str) {
    let visible = app.pull_requests().visible_items();
    let state = app.pull_requests();

    if visible.is_empty() {
        let mut items = Vec::new();
        let message = match state.load_status {
            crate::app::PullRequestLoadStatus::Loading if state.items.is_empty() => {
                "Loading open pull requests..."
            }
            crate::app::PullRequestLoadStatus::Failed if state.items.is_empty() => state
                .load_error
                .as_deref()
                .unwrap_or("No open pull requests"),
            _ if state.items.is_empty() => "No open pull requests",
            _ => "No pull requests match the current filter",
        };
        items.push(list_item(message, false));
        return (items, "PRs");
    }

    let selected = app.pull_requests().selected_visible_index();
    let items = visible
        .into_iter()
        .enumerate()
        .map(|(index, (_, pr))| render_pull_request_menu_item(pr, index == selected))
        .collect();
    (items, "PRs")
}

fn render_pull_request_menu_item(pr: &PullRequestSummary, selected: bool) -> ListItem<'static> {
    let header = if pr.title.is_empty() {
        format!("#{} (untitled)", pr.index)
    } else {
        format!("#{} {}", pr.index, pr.title)
    };

    let mut detail = format!("{} · {} · {} → {}", pr.state, pr.author, pr.head, pr.base);
    if !pr.updated.is_empty() {
        detail.push_str(&format!(" · {}", pr.updated));
    }
    if !pr.labels.is_empty() {
        detail.push_str(&format!(" · labels: {}", pr.labels.join(", ")));
    }

    let lines = vec![
        if selected {
            Line::from(header.clone()).bold().fg(colors::ACCENT)
        } else {
            Line::from(header).fg(colors::TEXT)
        },
        if selected {
            Line::from(detail).fg(colors::ACCENT)
        } else {
            Line::from(detail).fg(colors::MUTED)
        },
    ];

    ListItem::new(lines)
}

fn render_generate_menu(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = themed_block(
        focused_title("Changes", app.focus() == Focus::Menu),
        app.focus() == Focus::Menu,
    );
    let inner = block.inner(area);
    // inner_width accounts for the horizontal padding of 1 on each side applied by themed_block
    let inner_width = inner.width.saturating_sub(2) as usize;

    let (lines, selected_range) = render_generate_menu_lines(app, inner_width);
    let content_height = lines.len();
    let viewport_height = inner.height as usize;
    {
        let generate = app.generate_mut();
        if let Some((start, end)) = selected_range {
            generate
                .menu_scroll
                .ensure_visible(start, end, content_height, viewport_height);
        }
        generate.menu_scroll.clamp(content_height, viewport_height);
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((
            app.generate().menu_scroll.offset.min(u16::MAX as usize) as u16,
            0,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_generate_menu_lines(
    app: &App,
    inner_width: usize,
) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
    let revsets = &app.generate().revsets;
    let selected_idx = app.generate().selected_revset;
    let last_idx = revsets.len().saturating_sub(1);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut selected_range = None;

    for (index, revset) in revsets.iter().enumerate() {
        let row_start = lines.len();
        let is_selected = index == selected_idx;
        let row_lines = build_revset_row_lines(revset, is_selected, inner_width);
        lines.extend(row_lines);
        if is_selected {
            selected_range = Some((row_start, lines.len()));
        }

        // Separator between rows (not after the last)
        if index < last_idx {
            let sep = "─".repeat(inner_width);
            lines.push(Line::from(sep).fg(colors::BORDER));
        }
    }

    (lines, selected_range)
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

fn wrapped_content_height(lines: &[Line<'_>], width: usize) -> usize {
    if width == 0 {
        return 0;
    }

    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
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
fn compact_diff_stat(raw: &str) -> String {
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
    let bookmarks = revset.bookmarks();
    if !bookmarks.is_empty() {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(bookmarks.len() * 2);
        for (i, b) in bookmarks.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(", "));
            }
            spans.push(Span::styled(b.clone(), Style::new().bold()));
        }
        lines.push(Line::from(spans));
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

fn render_work(frame: &mut Frame, app: &mut App, area: Rect) {
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
        Screen::Generate => {
            let editing_text = app.focus() == Focus::Form
                && app.input_mode() == InputMode::Editing
                && !app.generate().selected_field().kind().is_picker();
            let (lines, selected_range, editor_row_range) =
                render_generate_fields(app, area, editing_text);
            let block = themed_block(
                focused_title("PR Form", app.focus() == Focus::Form),
                app.focus() == Focus::Form,
            );
            let inner = block.inner(area);
            let content_height = lines.len();
            let viewport_height = inner.height as usize;
            {
                let generate = app.generate_mut();
                if let Some((start, end)) = selected_range {
                    generate.form_scroll.ensure_visible(
                        start,
                        end,
                        content_height,
                        viewport_height,
                    );
                }
                generate.form_scroll.clamp(content_height, viewport_height);
            }
            let scroll_offset = app.generate().form_scroll.offset;

            let form = Paragraph::new(lines)
                .block(block)
                .scroll((scroll_offset.min(u16::MAX as usize) as u16, 0))
                .wrap(Wrap { trim: false });
            frame.render_widget(form, area);

            // Overlay the TextArea widget for the editing field.
            if editing_text && let Some((editor_start, editor_end)) = editor_row_range {
                let selected_field_id = app.generate().selected_field();
                if let Some(editor) = app.generate().form.field(selected_field_id).text_editor()
                    && let Some(editor_rect) =
                        compute_editor_rect(inner, editor_start, editor_end, scroll_offset)
                {
                    frame.render_widget(Clear, editor_rect);
                    frame.render_widget(editor, editor_rect);
                }
            }

            return;
        }
        Screen::PullRequests => render_pull_request_work(app),
        Screen::Issues => vec![
            Line::from("Manage Issues".bold()),
            Line::from(""),
            Line::from("Open issue list".fg(colors::MUTED)),
            Line::from("".to_string()),
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

fn render_pull_request_work<'a>(app: &'a App) -> Vec<Line<'a>> {
    let state = app.pull_requests();
    let visible = state.visible_items();
    let selected = state.selected_item().cloned();

    let mut lines = vec![
        Line::from("Manage PRs".bold()),
        Line::from(""),
        Line::from("Filter".fg(colors::MUTED)),
        Line::from(format!(
            "  {}",
            if state.filter.display_value().trim().is_empty() {
                "(type to filter)"
            } else {
                state.filter.display_value().trim()
            }
        )),
        Line::from(format!(
            "Status: {}  Visible: {}  Total: {}",
            state.load_status_label(),
            visible.len(),
            state.items.len()
        ))
        .fg(colors::MUTED),
    ];

    if let Some(error) = state.load_error.as_deref() {
        lines.push(Line::from(format!("Load error: {error}")).fg(colors::BAD));
    }

    match selected {
        Some(pr) => {
            lines.push(Line::from(""));
            lines.push(Line::from("Selected".fg(colors::MUTED)));
            lines.push(Line::from(format!("#{} {}", pr.index, pr.title)));
            lines.push(Line::from(format!(
                "{} · {} · {} → {}",
                pr.state, pr.author, pr.head, pr.base
            )));
            if !pr.updated.is_empty() {
                lines.push(Line::from(format!("Updated: {}", pr.updated)).fg(colors::MUTED));
            }
            if !pr.labels.is_empty() {
                lines.push(
                    Line::from(format!("Labels: {}", pr.labels.join(", "))).fg(colors::MUTED),
                );
            }
            match state.comment_phase {
                PrCommentPhase::Submitting => {
                    lines.push(Line::from(""));
                    lines.push(Line::from("Submitting comment...").fg(colors::ACCENT));
                }
                PrCommentPhase::Failed => {
                    lines.push(Line::from(""));
                    lines.push(
                        Line::from(format!(
                            "Comment failed: {}",
                            state.comment_error.as_deref().unwrap_or("unknown error")
                        ))
                        .fg(colors::BAD),
                    );
                }
                _ => {}
            }
        }
        None if state.load_status == crate::app::PullRequestLoadStatus::Loading => {
            lines.push(Line::from(""));
            lines.push(Line::from("Loading open pull requests...").fg(colors::ACCENT));
        }
        None if !state.items.is_empty() => {
            lines.push(Line::from(""));
            lines.push(Line::from("No pull requests match the current filter.").fg(colors::MUTED));
        }
        None if visible.is_empty()
            && state.load_status != crate::app::PullRequestLoadStatus::Failed =>
        {
            lines.push(Line::from(""));
            lines.push(Line::from("No open pull requests found.".fg(colors::MUTED)));
        }
        None => {}
    }

    lines
}

fn render_pull_request_preview(app: &App) -> Vec<Line<'static>> {
    let state = app.pull_requests();
    let filter = state.filter.display_value().trim().to_string();
    let filter_display = if filter.is_empty() {
        "(none)".to_string()
    } else {
        filter.clone()
    };
    let selected = state.selected_item().cloned();
    let has_selected = selected.is_some();
    let mut lines: Vec<Line<'static>> = vec![
        Line::from("Pull Request".bold()),
        Line::from(""),
        Line::from(format!("Filter: {filter_display}")).fg(colors::MUTED),
    ];

    if let Some(error) = state.load_error.as_deref() {
        lines.push(Line::from(format!("Load error: {error}")).fg(colors::BAD));
    }

    match selected {
        Some(pr) => {
            lines.push(Line::from(""));
            lines.push(Line::from(format!("#{} {}", pr.index, pr.title)).bold());
            lines.push(Line::from(format!("State: {}", pr.state)).fg(colors::MUTED));
            lines.push(Line::from(format!("Author: {}", pr.author)).fg(colors::MUTED));
            lines.push(Line::from(format!("Head: {}", pr.head)).fg(colors::MUTED));
            lines.push(Line::from(format!("Base: {}", pr.base)).fg(colors::MUTED));
            if !pr.url.is_empty() {
                lines.push(Line::from(format!("URL: {}", pr.url)).fg(colors::MUTED));
            }
            if !pr.updated.is_empty() {
                lines.push(Line::from(format!("Updated: {}", pr.updated)).fg(colors::MUTED));
            }
            if !pr.labels.is_empty() {
                lines.push(
                    Line::from(format!("Labels: {}", pr.labels.join(", "))).fg(colors::MUTED),
                );
            }
            lines.push(Line::from(""));
            lines.push(Line::from("Body".bold()));
            if pr.body.trim().is_empty() {
                lines.push(Line::from("  (empty body)").fg(colors::MUTED));
            } else {
                for line in pr.body.lines() {
                    lines.push(Line::from(format!("  {line}")));
                }
            }
        }
        None if state.load_status == crate::app::PullRequestLoadStatus::Loading => {
            lines.push(Line::from(""));
            lines.push(Line::from("Loading open pull requests...").fg(colors::ACCENT));
        }
        None if !state.items.is_empty() => {
            lines.push(Line::from(""));
            lines.push(Line::from("No pull requests match the current filter.").fg(colors::MUTED));
        }
        None if state.load_status == crate::app::PullRequestLoadStatus::Failed => {
            lines.push(Line::from(""));
            lines.push(
                Line::from(
                    state
                        .load_error
                        .as_deref()
                        .unwrap_or("Failed to load open pull requests")
                        .to_string(),
                )
                .fg(colors::BAD),
            );
        }
        None => {
            lines.push(Line::from(""));
            lines.push(Line::from("No open pull requests available.").fg(colors::MUTED));
        }
    }

    if has_selected {
        lines.push(Line::from(""));
        lines.push(
            Line::from(format!(
                "Visible: {}  Selected: {}",
                state.visible_count(),
                state.selected_visible_index() + 1
            ))
            .fg(colors::MUTED),
        );
    }

    lines
}

fn render_pr_comment_modal(frame: &mut Frame, app: &App, frame_area: Rect) {
    let state = app.pull_requests();
    if !matches!(
        state.comment_phase,
        PrCommentPhase::Editing | PrCommentPhase::Submitting | PrCommentPhase::Failed
    ) {
        return;
    }

    let pr_title = state
        .selected_item()
        .map(|pr| format!("#{} {}", pr.index, pr.title))
        .unwrap_or_else(|| "(no PR selected)".into());

    let modal_width = (70u16).min(frame_area.width.saturating_sub(8)).max(30);
    // Height: title line + blank + PR name + blank + input label + input line +
    //         blank + status/error line + blank + hint line + 2 border rows = 12
    let modal_height = 12u16.min(frame_area.height.saturating_sub(4).max(6));
    let modal_rect = centered_rect(modal_width, modal_height, frame_area);

    frame.render_widget(Clear, modal_rect);

    let phase_label = match state.comment_phase {
        PrCommentPhase::Submitting => "SUBMITTING",
        PrCommentPhase::Failed => "FAILED — edit and retry",
        _ => "Add Comment",
    };

    let title = Line::from(phase_label.bold().fg(colors::ACCENT));
    let block = themed_block(title, true);
    let inner = block.inner(modal_rect);
    frame.render_widget(block, modal_rect);

    let cursor_pos = state.comment_cursor;
    let buf = &state.comment_buffer;

    // Build a visual representation of the input with a cursor marker.
    let before_cursor = &buf[..cursor_pos];
    let after_cursor = &buf[cursor_pos..];
    let cursor_char = after_cursor.chars().next().unwrap_or(' ');
    let rest_after_cursor = after_cursor
        .char_indices()
        .nth(1)
        .map(|(i, _)| &after_cursor[i..])
        .unwrap_or("");

    let input_line = Line::from(vec![
        Span::raw(before_cursor.to_string()),
        Span::styled(
            cursor_char.to_string(),
            Style::new().fg(colors::BASE).bg(colors::ACCENT),
        ),
        Span::raw(rest_after_cursor.to_string()),
    ]);

    let pr_name_line = truncate_chars(&pr_title, inner.width as usize);

    let mut content_lines: Vec<Line<'static>> = vec![
        Line::from(pr_name_line.fg(colors::MUTED)),
        Line::from(""),
        Line::from("Comment:".fg(colors::TEXT)),
        input_line,
        Line::from(""),
    ];

    if let Some(error) = state.comment_error.as_deref() {
        content_lines.push(Line::from(error.to_string()).fg(colors::BAD));
    } else if state.comment_phase == PrCommentPhase::Submitting {
        content_lines.push(Line::from("Submitting...").fg(colors::ACCENT));
    } else {
        content_lines.push(Line::from(""));
    }

    content_lines.push(Line::from(""));
    let hint = if state.comment_phase == PrCommentPhase::Submitting {
        Line::from("Please wait...".fg(colors::MUTED))
    } else {
        Line::from(vec![
            " Enter ".bold().fg(colors::ACCENT),
            "submit ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "cancel".fg(colors::MUTED),
        ])
    };
    content_lines.push(hint);

    frame.render_widget(Paragraph::new(content_lines), inner);
}

#[allow(clippy::type_complexity)]
fn render_generate_fields(
    app: &App,
    area: Rect,
    editing_text: bool,
) -> (
    Vec<Line<'static>>,
    Option<(usize, usize)>,
    Option<(usize, usize)>,
) {
    let total = FieldId::ALL.len();
    let last = total.saturating_sub(1);
    let sep_width = area.width.saturating_sub(6) as usize;
    let separator = format!("  {}  ", "─".repeat(sep_width));
    let mut lines = Vec::new();
    let mut selected_range = None;
    let mut editor_row_range = None;

    for (index, field_id) in FieldId::ALL.iter().enumerate() {
        let start = lines.len();
        let is_selected = index == app.generate().selected_field;
        let is_focused = is_selected && app.focus() == Focus::Form;
        let editing_this_field = is_selected && editing_text;
        let (mut field_lines, edit_range) = render_generate_field(
            app.generate(),
            *field_id,
            is_selected,
            is_focused,
            total,
            editing_this_field,
        );
        lines.append(&mut field_lines);
        if is_selected {
            selected_range = Some((start, lines.len()));
            if let Some((rel_start, rel_end)) = edit_range {
                editor_row_range = Some((start + rel_start, start + rel_end));
            }
        }
        if index < last {
            lines.push(Line::from(separator.clone()).fg(colors::BORDER));
        }
    }

    (lines, selected_range, editor_row_range)
}

fn generate_work_title(phase: GeneratePhase) -> &'static str {
    match phase {
        GeneratePhase::CollectingContext => "Collecting Context",
        GeneratePhase::ContextReady => "Context Ready",
        GeneratePhase::Generating => "Generating Draft",
        GeneratePhase::DraftReady => "Draft Review",
        GeneratePhase::CheckingFreshness => "Verifying Repo Context",
        GeneratePhase::Confirming => "Execution Preview",
        GeneratePhase::Executing => "Executing",
        GeneratePhase::Complete => "Execution Complete",
        GeneratePhase::Failed => "Workflow Failed",
        GeneratePhase::SelectingRevset | GeneratePhase::EditingForm => "PR Form",
    }
}

fn render_preview(frame: &mut Frame, app: &mut App, area: Rect) {
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
        Screen::Generate => {
            let lines = render_generate_preview(&*app);
            let block = themed_block(
                focused_title("Preview", app.focus() == Focus::Preview),
                app.focus() == Focus::Preview,
            );
            let inner = block.inner(area);
            let content_height = wrapped_content_height(&lines, inner.width as usize);
            let viewport_height = inner.height as usize;
            {
                let generate = app.generate_mut();
                generate
                    .preview_scroll
                    .clamp(content_height, viewport_height);
            }

            let preview = Paragraph::new(lines)
                .block(block)
                .scroll((
                    app.generate().preview_scroll.offset.min(u16::MAX as usize) as u16,
                    0,
                ))
                .wrap(Wrap { trim: false });
            frame.render_widget(preview, area);
            return;
        }
        Screen::PullRequests => {
            let lines = render_pull_request_preview(app);
            let block = themed_block(
                focused_title("Preview", app.focus() == Focus::Preview),
                app.focus() == Focus::Preview,
            );
            let inner = block.inner(area);
            let content_height = wrapped_content_height(&lines, inner.width as usize);
            let viewport_height = inner.height as usize;
            {
                let state = app.pull_requests_mut();
                state.preview_scroll.clamp(content_height, viewport_height);
            }

            let preview = Paragraph::new(lines)
                .block(block)
                .scroll((
                    app.pull_requests()
                        .preview_scroll
                        .offset
                        .min(u16::MAX as usize) as u16,
                    0,
                ))
                .wrap(Wrap { trim: false });
            frame.render_widget(preview, area);
            return;
        }
        Screen::Issues => {
            let lines = vec![
                Line::from("Issue Preview".bold()),
                Line::from(""),
                Line::from("Selected issue body and comments will appear here."),
                Line::from("Esc returns to Landing.".fg(colors::MUTED)),
            ];
            let block = themed_block(
                focused_title("Preview", app.focus() == Focus::Preview),
                app.focus() == Focus::Preview,
            );
            let inner = block.inner(area);
            let content_height = wrapped_content_height(&lines, inner.width as usize);
            let viewport_height = inner.height as usize;
            {
                let state = app.issues_mut();
                state.preview_scroll.clamp(content_height, viewport_height);
            }

            let preview = Paragraph::new(lines)
                .block(block)
                .scroll((
                    app.issues().preview_scroll.offset.min(u16::MAX as usize) as u16,
                    0,
                ))
                .wrap(Wrap { trim: false });
            frame.render_widget(preview, area);
            return;
        }
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
    } else if app.screen() == Screen::PullRequests {
        let pr = app.pull_requests();
        let selected = if pr.visible_count() == 0 {
            0
        } else {
            pr.selected_visible_index() + 1
        };
        raw_segments.push(format!(" prs:{} ", pr.load_status_label()).fg(colors::MUTED));
        raw_segments.push(format!(" prs:{selected}/{} ", pr.visible_count()).fg(colors::MUTED));
        if let Some(msg) = app.status_message() {
            let is_error = msg.starts_with("error:");
            let span: Span<'static> = if is_error {
                Span::styled(format!(" {msg} "), Style::new().fg(colors::BAD))
            } else {
                Span::styled(format!(" {msg} "), Style::new().fg(colors::GOOD))
            };
            raw_segments.push(span);
        }
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
        Screen::Generate
            if app.input_mode() == InputMode::Editing
                && app.generate().selected_field().kind().is_picker() =>
        {
            let is_multi = app.generate().selected_field().kind().is_multi_select();
            if is_multi {
                Line::from(vec![
                    " ↑↓ ".bold().fg(colors::ACCENT),
                    "move ".fg(colors::MUTED),
                    " Space ".bold().fg(colors::ACCENT),
                    "toggle ".fg(colors::MUTED),
                    " Enter ".bold().fg(colors::ACCENT),
                    "ok ".fg(colors::MUTED),
                    " Esc ".bold().fg(colors::ACCENT),
                    "cancel ".fg(colors::MUTED),
                ])
            } else {
                Line::from(vec![
                    " ↑↓ ".bold().fg(colors::ACCENT),
                    "move ".fg(colors::MUTED),
                    " type ".bold().fg(colors::ACCENT),
                    "filter ".fg(colors::MUTED),
                    " Enter ".bold().fg(colors::ACCENT),
                    "ok ".fg(colors::MUTED),
                    " Esc ".bold().fg(colors::ACCENT),
                    "cancel ".fg(colors::MUTED),
                ])
            }
        }
        Screen::Generate if app.input_mode() == InputMode::Editing => Line::from(vec![
            " cursor ".bold().fg(colors::ACCENT),
            "editing active ".fg(colors::MUTED),
            " Enter ".bold().fg(colors::ACCENT),
            "save single-line / newline description ".fg(colors::MUTED),
            " Ctrl+S ".bold().fg(colors::ACCENT),
            "commit description ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "cancel ".fg(colors::MUTED),
        ]),
        Screen::Generate if app.input_mode() == InputMode::Confirm => Line::from(vec![
            " Enter ".bold().fg(colors::ACCENT),
            "execute ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "cancel ".fg(colors::MUTED),
        ]),
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
        Screen::Generate => generate_footer_hints(app.focus(), app.generate().draft.is_some()),
        Screen::PullRequests
            if matches!(
                app.pull_requests().comment_phase,
                PrCommentPhase::Editing | PrCommentPhase::Failed
            ) =>
        {
            Line::from(vec![
                " cursor ".bold().fg(colors::ACCENT),
                "editing comment ".fg(colors::MUTED),
                " Enter ".bold().fg(colors::ACCENT),
                "submit ".fg(colors::MUTED),
                " Esc ".bold().fg(colors::ACCENT),
                "cancel ".fg(colors::MUTED),
            ])
        }
        Screen::PullRequests if app.pull_requests().comment_phase == PrCommentPhase::Submitting => {
            Line::from(vec![" submitting comment... ".fg(colors::ACCENT)])
        }
        Screen::PullRequests if app.input_mode() == InputMode::Editing => Line::from(vec![
            " cursor ".bold().fg(colors::ACCENT),
            "editing filter ".fg(colors::MUTED),
            " Enter ".bold().fg(colors::ACCENT),
            "save ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "cancel ".fg(colors::MUTED),
        ]),
        Screen::PullRequests if app.pull_requests().selected_item().is_some() => Line::from(vec![
            " j/k ".bold().fg(colors::ACCENT),
            "move ".fg(colors::MUTED),
            " h/l/tab ".bold().fg(colors::ACCENT),
            "panes ".fg(colors::MUTED),
            " c ".bold().fg(colors::ACCENT),
            "comment ".fg(colors::MUTED),
            " o ".bold().fg(colors::ACCENT),
            "open ".fg(colors::MUTED),
            " y ".bold().fg(colors::ACCENT),
            "yank url ".fg(colors::MUTED),
            " Enter/i ".bold().fg(colors::ACCENT),
            "edit filter ".fg(colors::MUTED),
            " r ".bold().fg(colors::ACCENT),
            "refresh ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Screen::PullRequests => Line::from(vec![
            " j/k ".bold().fg(colors::ACCENT),
            "move ".fg(colors::MUTED),
            " h/l/tab ".bold().fg(colors::ACCENT),
            "panes ".fg(colors::MUTED),
            " Enter/i ".bold().fg(colors::ACCENT),
            "edit filter ".fg(colors::MUTED),
            " r ".bold().fg(colors::ACCENT),
            "refresh ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Screen::Issues => Line::from(vec![
            " Enter ".bold().fg(colors::ACCENT),
            "select ".fg(colors::MUTED),
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

/// Returns the rendered lines for a single form field, plus an optional
/// `(relative_start, relative_end)` range indicating which rows within the
/// returned slice are blank placeholders reserved for the TextArea widget
/// overlay.  The range is `Some(...)` only when `editing` is true and the
/// field is a text field.
fn render_generate_field(
    generate: &GenerateState,
    field_id: FieldId,
    selected: bool,
    focused: bool,
    total_fields: usize,
    editing: bool,
) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
    let field = generate.form.field(field_id);
    let label = field_id.label();
    let value = field.display_value().to_string();
    let error_count = field.errors().len();
    let marker = if selected { "▶" } else { " " };
    let is_multiline = field_id.kind().is_multiline();
    let is_picker = field_id.kind().is_picker();
    let index_suffix = if focused {
        let n = generate.selected_field + 1;
        format!("  ({n}/{})", total_fields)
    } else {
        String::new()
    };

    // When actively editing a text field, the value is rendered by the
    // TextArea widget overlay, not inline.  The header omits the value for
    // single-line fields and blank placeholder rows are inserted for the
    // editor widget to overdraw.
    let editing_text = editing && !is_picker;

    let header = if is_multiline {
        if error_count > 0 {
            format!("{marker} {label} ({error_count} errors){index_suffix}")
        } else {
            format!("{marker} {label}{index_suffix}")
        }
    } else if editing_text {
        // Single-line text field being edited: omit value from header so the
        // textarea widget below the header row shows the live value.
        if error_count > 0 {
            format!("{marker} {label}: ({error_count} errors){index_suffix}")
        } else {
            format!("{marker} {label}:{index_suffix}")
        }
    } else if error_count > 0 {
        format!("{marker} {label}: {value} ({error_count} errors){index_suffix}")
    } else {
        format!("{marker} {label}: {value}{index_suffix}")
    };

    let mut lines = Vec::with_capacity(1 + error_count + usize::from(is_multiline));
    if focused {
        lines.push(Line::from(header.bold().fg(colors::ACCENT)));
    } else {
        lines.push(Line::from(header.fg(colors::MUTED)));
    }

    let mut edit_range: Option<(usize, usize)> = None;

    if editing_text {
        // Insert blank placeholder rows that will be overdrawn by the
        // TextArea widget.  The range is relative to the start of `lines`.
        let placeholder_count = if is_multiline {
            DESCRIPTION_FIELD_DISPLAY_LINES
        } else {
            1
        };
        let rel_start = lines.len();
        for _ in 0..placeholder_count {
            lines.push(Line::from(""));
        }
        edit_range = Some((rel_start, rel_start + placeholder_count));
    } else if is_multiline {
        let field_lines = bounded_multiline_field_lines(&value, DESCRIPTION_FIELD_DISPLAY_LINES);
        if field_lines.is_empty() {
            lines.push(Line::from("    (empty)").fg(colors::MUTED));
        } else {
            for line in field_lines {
                lines.push(Line::from(format!("    {line}")));
            }
            let total_lines = value.lines().count();
            if total_lines > DESCRIPTION_FIELD_DISPLAY_LINES {
                lines.push(
                    Line::from(format!(
                        "    ... {} more lines",
                        total_lines - DESCRIPTION_FIELD_DISPLAY_LINES
                    ))
                    .fg(colors::MUTED),
                );
            }
        }
    } else if is_picker {
        let selected_values = field.picker_selected_values();
        let selected_summary = if selected_values.is_empty() {
            "(none)".to_string()
        } else {
            selected_values.join(", ")
        };

        if field.picker_is_editing() {
            // While the modal is open, collapse the inline listing to a single
            // summary row so the form pane does not double-render option lists.
            lines.push(Line::from(format!("    {selected_summary}  (editing…)")).fg(colors::MUTED));
        } else {
            lines.push(Line::from(format!("    selected: {selected_summary}")));
            if field.picker_options().is_empty() {
                lines.push(Line::from("    (no options loaded)").fg(colors::MUTED));
            }
        }
    }

    for error in field.errors() {
        lines.push(Line::from(format!("    - {error}")).fg(colors::BAD));
    }

    (lines, edit_range)
}

fn bounded_multiline_field_lines(value: &str, max_lines: usize) -> Vec<&str> {
    if value.trim().is_empty() || max_lines == 0 {
        return Vec::new();
    }
    value.lines().take(max_lines).collect()
}

/// Compute the sub-Rect within `inner` that corresponds to the placeholder
/// rows `[editor_start, editor_end)` in the content, after applying
/// `scroll_offset`.  Returns `None` when the editor rows are entirely
/// outside the visible viewport.
fn compute_editor_rect(
    inner: Rect,
    editor_start: usize,
    editor_end: usize,
    scroll_offset: usize,
) -> Option<Rect> {
    let viewport_height = inner.height as usize;
    if viewport_height == 0 || editor_end <= scroll_offset {
        return None;
    }

    // Row in the viewport (0-indexed) where the editor area begins.
    let vis_start = editor_start.saturating_sub(scroll_offset);
    let vis_end = editor_end.saturating_sub(scroll_offset);

    if vis_start >= viewport_height {
        return None;
    }

    let clamped_start = vis_start.min(viewport_height - 1);
    let clamped_end = vis_end.min(viewport_height);
    if clamped_end <= clamped_start {
        return None;
    }

    let y = inner.y + clamped_start as u16;
    let height = (clamped_end - clamped_start) as u16;

    Some(Rect {
        x: inner.x,
        y,
        width: inner.width,
        height,
    })
}

fn render_generate_preview(app: &App) -> Vec<Line<'static>> {
    let generate = app.generate();
    let revset = generate.selected_revset();

    // Always-visible identifier block at the top.
    let mut lines = render_change_identifier(revset);

    // Base branch as a muted footer line of the identifier block.
    lines.push(Line::from(
        format!("base: {}", app.repo().base_branch.name).fg(colors::MUTED),
    ));

    // Phase-specific section.
    let phase_title = generate_work_title(generate.phase);
    lines.extend(render_section_header(phase_title));

    match generate.phase {
        GeneratePhase::CollectingContext => {
            // Status
            lines.push(Line::from("Collecting context…".fg(colors::ACCENT)));
            lines.push(Line::from(
                format!(
                    "base: {} (branch ref or change_id)",
                    generate.form.base.display_value()
                )
                .fg(colors::MUTED),
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

// ---------------------------------------------------------------------------
// Picker modal helpers
// ---------------------------------------------------------------------------

/// Return a `Rect` centered inside `area` with the given width and height,
/// clamped so it never exceeds `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// The maximum number of option rows the modal will show before adding a
/// `(… N more)` muted line.
const PICKER_MODAL_MAX_ROWS: usize = 10;

/// Compute a visible slice of `options` around `highlighted`, limited to
/// `max_rows` entries.  Returns the slice and a count of options that were
/// trimmed off the end.
fn picker_visible_slice(
    options: &[PickerOptionView],
    highlighted: usize,
    max_rows: usize,
) -> (&[PickerOptionView], usize) {
    if options.is_empty() || max_rows == 0 {
        return (&[], 0);
    }
    let total = options.len();
    if total <= max_rows {
        return (options, 0);
    }
    // Center the window around `highlighted`, staying in-bounds.
    let half = max_rows / 2;
    let start = if highlighted >= half {
        (highlighted - half).min(total - max_rows)
    } else {
        0
    };
    let end = (start + max_rows).min(total);
    let remaining = total.saturating_sub(end);
    (&options[start..end], remaining)
}

/// Render a centered modal popup for the actively-editing picker field, if the
/// gating conditions are met:
///
/// - `Screen::Generate` is active.
/// - `Focus::Form` is held.
/// - The selected field is a picker.
/// - The selected picker field is currently in editing mode.
fn render_picker_modal(frame: &mut Frame, app: &App, frame_area: Rect) {
    // Gate: Generate screen, Form focus, editing a picker.
    if app.screen() != Screen::Generate || app.focus() != Focus::Form {
        return;
    }
    if app.input_mode() != InputMode::Editing {
        return;
    }
    let field_id = app.generate().selected_field();
    if !field_id.kind().is_picker() {
        return;
    }
    let field = app.generate().form.field(field_id);
    if !field.picker_is_editing() {
        return;
    }

    let label = field_id.label();
    let filter = field.picker_filter().unwrap_or("").trim();
    let is_multi = field_id.kind().is_multi_select();

    let visible_options = field.picker_visible_options();
    let highlighted = visible_options
        .iter()
        .position(|o| o.highlighted)
        .unwrap_or_default();

    // Layout: 1 (filter row) + up to PICKER_MODAL_MAX_ROWS (options) + 1
    // (footer) = at most PICKER_MODAL_MAX_ROWS + 2 inner rows, plus 2 for
    // border = PICKER_MODAL_MAX_ROWS + 4.
    let option_display_rows = visible_options.len().min(PICKER_MODAL_MAX_ROWS);
    let has_more_indicator = visible_options.len() > PICKER_MODAL_MAX_ROWS;
    let inner_height = 1 // filter
        + option_display_rows
        + usize::from(has_more_indicator) // "(… N more)" line
        + 1; // footer
    let modal_height = (inner_height + 2) as u16; // +2 for block border
    let modal_width = (60u16).min(frame_area.width.saturating_sub(8)).max(20);
    let clamped_height = modal_height.min(frame_area.height.saturating_sub(4).max(6));

    let modal_rect = centered_rect(modal_width, clamped_height, frame_area);

    // Clear the background.
    frame.render_widget(Clear, modal_rect);

    // Outer block (always "focused" because the modal owns input).  Reuse
    // `themed_block` so the modal stays in step with other panes' chrome.
    let title = Line::from(label.bold().fg(colors::ACCENT));
    let block = themed_block(title, true);

    let inner = block.inner(modal_rect);
    frame.render_widget(block, modal_rect);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Build content lines inside the modal.
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Filter row.
    let filter_display = if filter.is_empty() {
        "(none)".to_string()
    } else {
        filter.to_string()
    };
    lines.push(Line::from(format!("Filter: {filter_display}")).fg(colors::MUTED));

    // Option rows (sliced around highlighted).
    let (slice, remaining) =
        picker_visible_slice(&visible_options, highlighted, PICKER_MODAL_MAX_ROWS);
    for option in slice {
        let prefix = if option.highlighted { "▶" } else { " " };
        let selection = match (is_multi, option.selected) {
            (true, true) => "[x]",
            (false, true) => "[•]",
            (_, false) => "[ ]",
        };
        let mut label_text = format!("{prefix} {selection} {}", option.label);
        if !option.enabled {
            label_text.push_str(" (disabled)");
        }
        let styled = if option.highlighted {
            Line::from(label_text).fg(colors::ACCENT)
        } else if !option.enabled {
            Line::from(label_text).fg(colors::MUTED)
        } else {
            Line::from(label_text)
        };
        lines.push(styled);
    }

    if remaining > 0 {
        lines.push(Line::from(format!("(… {remaining} more)")).fg(colors::MUTED));
    }

    // Footer key-hint row.
    let footer_text = if inner.width < 40 {
        if is_multi {
            "↑↓ · Spc tog · Ent ok · Esc x".to_string()
        } else {
            "↑↓ · Ent ok · Esc x".to_string()
        }
    } else if is_multi {
        "↑↓ move · Space toggle · Enter ok · Esc cancel".to_string()
    } else {
        "↑↓ move · type filter · Enter ok · Esc cancel".to_string()
    };
    lines.push(Line::from(footer_text).fg(colors::MUTED));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Returns the footer hint line for the Generate screen's default (non-editing,
/// non-confirm, non-terminal-phase) state, varying by focused pane and whether
/// a generated draft is currently present.
fn generate_footer_hints(focus: Focus, has_draft: bool) -> Line<'static> {
    match focus {
        Focus::Menu => Line::from(vec![
            " ↑/↓ ".bold().fg(colors::ACCENT),
            "select ".fg(colors::MUTED),
            " Enter ".bold().fg(colors::ACCENT),
            "pick revset ".fg(colors::MUTED),
            " r ".bold().fg(colors::ACCENT),
            "refresh ".fg(colors::MUTED),
            " Tab ".bold().fg(colors::ACCENT),
            "→ Form ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Focus::Form => Line::from(vec![
            " ↑/↓ ".bold().fg(colors::ACCENT),
            "field ".fg(colors::MUTED),
            " Enter/i ".bold().fg(colors::ACCENT),
            "edit ".fg(colors::MUTED),
            " g ".bold().fg(colors::ACCENT),
            "generate ".fg(colors::MUTED),
            " Tab ".bold().fg(colors::ACCENT),
            "→ Preview ".fg(colors::MUTED),
            " Shift+Tab ".bold().fg(colors::ACCENT),
            "→ Menu ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Focus::Preview if !has_draft => Line::from(vec![
            " ↑/↓ ".bold().fg(colors::ACCENT),
            "scroll ".fg(colors::MUTED),
            " Tab ".bold().fg(colors::ACCENT),
            "→ Menu ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
        Focus::Preview => Line::from(vec![
            " ↑/↓ ".bold().fg(colors::ACCENT),
            "scroll ".fg(colors::MUTED),
            " p ".bold().fg(colors::ACCENT),
            "manifest/raw ".fg(colors::MUTED),
            " g ".bold().fg(colors::ACCENT),
            "regenerate ".fg(colors::MUTED),
            " c ".bold().fg(colors::ACCENT),
            "confirm ".fg(colors::MUTED),
            " Tab ".bold().fg(colors::ACCENT),
            "→ Menu ".fg(colors::MUTED),
            " Esc ".bold().fg(colors::ACCENT),
            "back ".fg(colors::MUTED),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_multiline_field_lines, centered_rect, compact_diff_stat, compute_editor_rect,
        generate_footer_hints, is_jj_default_description, picker_visible_slice, truncate_chars,
        wrap_chars, wrapped_content_height,
    };
    use crate::generate::{Focus, PickerOptionView};
    use ratatui::layout::Rect;
    use ratatui::text::{Line, Span};

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
    fn wrapped_content_height_accounts_for_wrapped_preview_lines() {
        let lines = vec![
            Line::from("abcdefghij"),
            Line::from(vec![Span::raw("ab"), Span::raw("cdef")]),
            Line::from(""),
        ];

        assert_eq!(wrapped_content_height(&lines, 4), 6);
        assert_eq!(wrapped_content_height(&lines, 0), 0);
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

    #[test]
    fn bounded_multiline_field_lines_clamps_description_preview() {
        let lines = bounded_multiline_field_lines("a\nb\nc\nd", 2);

        assert_eq!(lines, vec!["a", "b"]);
    }

    #[test]
    fn bounded_multiline_field_lines_hides_empty_description() {
        assert!(bounded_multiline_field_lines(" \n ", 6).is_empty());
    }

    fn inner(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn compute_editor_rect_no_scroll_single_row_in_viewport() {
        // Editor rows 1..2 (1 row), no scroll, viewport height 10.
        let rect = compute_editor_rect(inner(2, 3, 30, 10), 1, 2, 0);
        let r = rect.expect("should be Some");
        assert_eq!(r.y, 3 + 1); // inner.y + vis_start
        assert_eq!(r.height, 1);
        assert_eq!(r.x, 2);
        assert_eq!(r.width, 30);
    }

    #[test]
    fn compute_editor_rect_multiline_no_scroll() {
        // Editor rows 1..7 (6 rows = DESCRIPTION_FIELD_DISPLAY_LINES), no scroll.
        let rect = compute_editor_rect(inner(0, 0, 40, 20), 1, 7, 0);
        let r = rect.expect("should be Some");
        assert_eq!(r.y, 1);
        assert_eq!(r.height, 6);
    }

    #[test]
    fn compute_editor_rect_scrolled_partially_visible() {
        // Editor rows 3..9 with scroll offset 5: visible rows 0..4 of viewport.
        // vis_start = 3 - 5 = 0 (clamped), vis_end = 9 - 5 = 4.
        let rect = compute_editor_rect(inner(0, 0, 40, 10), 3, 9, 5);
        let r = rect.expect("should be Some");
        assert_eq!(r.y, 0);
        assert_eq!(r.height, 4);
    }

    #[test]
    fn compute_editor_rect_entirely_above_viewport_returns_none() {
        // Editor rows 0..2 with scroll offset 5: entirely scrolled past.
        assert!(compute_editor_rect(inner(0, 0, 40, 10), 0, 2, 5).is_none());
    }

    #[test]
    fn compute_editor_rect_entirely_below_viewport_returns_none() {
        // Editor rows 15..17 with viewport height 10 and no scroll.
        assert!(compute_editor_rect(inner(0, 0, 40, 10), 15, 17, 0).is_none());
    }

    #[test]
    fn compute_editor_rect_zero_height_viewport_returns_none() {
        assert!(compute_editor_rect(inner(0, 0, 40, 0), 0, 1, 0).is_none());
    }

    // ---------------------------------------------------------------------------
    // centered_rect tests
    // ---------------------------------------------------------------------------

    #[test]
    fn centered_rect_fits_inside_area() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 40,
        };
        let r = centered_rect(60, 20, area);
        // Must be contained within the area.
        assert!(r.x >= area.x);
        assert!(r.y >= area.y);
        assert!(r.x + r.width <= area.x + area.width);
        assert!(r.y + r.height <= area.y + area.height);
        assert_eq!(r.width, 60);
        assert_eq!(r.height, 20);
    }

    #[test]
    fn centered_rect_is_centered() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 40,
        };
        let r = centered_rect(60, 20, area);
        // Horizontal center: (80 - 60) / 2 = 10.
        assert_eq!(r.x, 10);
        // Vertical center: (40 - 20) / 2 = 10.
        assert_eq!(r.y, 10);
    }

    #[test]
    fn centered_rect_clamps_to_area_when_larger() {
        let area = Rect {
            x: 5,
            y: 3,
            width: 20,
            height: 10,
        };
        let r = centered_rect(100, 100, area);
        assert_eq!(r.x, area.x);
        assert_eq!(r.y, area.y);
        assert_eq!(r.width, area.width);
        assert_eq!(r.height, area.height);
    }

    // ---------------------------------------------------------------------------
    // picker_visible_slice tests
    // ---------------------------------------------------------------------------

    fn make_options(count: usize) -> Vec<PickerOptionView> {
        (0..count)
            .map(|i| PickerOptionView {
                label: format!("option {i}"),
                value: format!("v{i}"),
                enabled: true,
                selected: false,
                highlighted: false,
            })
            .collect()
    }

    #[test]
    fn picker_visible_slice_all_fit_when_small() {
        let opts = make_options(5);
        let (slice, remaining) = picker_visible_slice(&opts, 2, 10);
        assert_eq!(slice.len(), 5);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn picker_visible_slice_windows_around_highlighted() {
        let opts = make_options(20);
        // max_rows=5, highlighted=10 → window should include index 10.
        let (slice, remaining) = picker_visible_slice(&opts, 10, 5);
        assert_eq!(slice.len(), 5);
        // Remaining = 20 - (start + 5); window starts around 10-2=8, so end=13, remaining=7.
        assert!(remaining > 0);
        // The highlighted index (10) should be inside the slice.
        let slice_values: Vec<&str> = slice.iter().map(|o| o.value.as_str()).collect();
        assert!(
            slice_values.contains(&"v10"),
            "highlighted not in slice: {slice_values:?}"
        );
    }

    #[test]
    fn picker_visible_slice_highlight_near_start_stays_in_bounds() {
        let opts = make_options(20);
        let (slice, _remaining) = picker_visible_slice(&opts, 1, 5);
        assert_eq!(slice.len(), 5);
        // Window starts at 0 because highlighted=1 < half=2.
        assert_eq!(slice[0].value, "v0");
    }

    #[test]
    fn picker_visible_slice_highlight_near_end_stays_in_bounds() {
        let opts = make_options(20);
        let (slice, remaining) = picker_visible_slice(&opts, 19, 5);
        assert_eq!(slice.len(), 5);
        // Window must end at 20.
        assert_eq!(remaining, 0);
        assert_eq!(slice.last().unwrap().value, "v19");
    }

    #[test]
    fn picker_visible_slice_empty_input_returns_empty() {
        let opts: Vec<PickerOptionView> = Vec::new();
        let (slice, remaining) = picker_visible_slice(&opts, 0, 10);
        assert!(slice.is_empty());
        assert_eq!(remaining, 0);
    }

    #[test]
    fn picker_visible_slice_zero_max_rows_returns_empty() {
        let opts = make_options(5);
        let (slice, remaining) = picker_visible_slice(&opts, 2, 0);
        assert!(slice.is_empty());
        assert_eq!(remaining, 0);
    }

    // ---------------------------------------------------------------------------
    // generate_footer_hints tests
    // ---------------------------------------------------------------------------

    fn line_text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn generate_footer_hints_menu_contains_r_refresh() {
        let line = generate_footer_hints(Focus::Menu, false);
        let text = line_text(&line);
        assert!(
            text.contains("r") && text.contains("refresh"),
            "Menu hints should contain 'r refresh', got: {text:?}"
        );
        // Pane-exclusive keys must not appear in Menu hints.
        assert!(
            !text.contains("generate"),
            "Menu hints must not contain 'generate', got: {text:?}"
        );
        assert!(
            !text.contains("manifest/raw"),
            "Menu hints must not contain 'manifest/raw', got: {text:?}"
        );
    }

    #[test]
    fn generate_footer_hints_menu_no_draft_still_same() {
        // The Menu arm doesn't vary with has_draft.
        let with_draft = line_text(&generate_footer_hints(Focus::Menu, true));
        let without_draft = line_text(&generate_footer_hints(Focus::Menu, false));
        assert_eq!(with_draft, without_draft);
    }

    #[test]
    fn generate_footer_hints_form_contains_g_generate() {
        let line = generate_footer_hints(Focus::Form, false);
        let text = line_text(&line);
        assert!(
            text.contains("generate"),
            "Form hints should contain 'generate', got: {text:?}"
        );
        // Refresh is Menu-only.
        assert!(
            !text.contains("refresh"),
            "Form hints must not contain 'refresh', got: {text:?}"
        );
        assert!(
            !text.contains("manifest/raw"),
            "Form hints must not contain 'manifest/raw', got: {text:?}"
        );
    }

    #[test]
    fn generate_footer_hints_preview_no_draft_omits_draft_keys() {
        let line = generate_footer_hints(Focus::Preview, false);
        let text = line_text(&line);
        assert!(
            !text.contains("manifest/raw"),
            "Preview without draft must not show 'manifest/raw', got: {text:?}"
        );
        assert!(
            !text.contains("confirm"),
            "Preview without draft must not show 'confirm', got: {text:?}"
        );
        assert!(
            !text.contains("regenerate"),
            "Preview without draft must not show 'regenerate', got: {text:?}"
        );
    }

    #[test]
    fn generate_footer_hints_preview_with_draft_shows_manifest_confirm_regenerate() {
        let line = generate_footer_hints(Focus::Preview, true);
        let text = line_text(&line);
        assert!(
            text.contains("manifest/raw"),
            "Preview with draft must show 'manifest/raw', got: {text:?}"
        );
        assert!(
            text.contains("regenerate"),
            "Preview with draft must show 'regenerate', got: {text:?}"
        );
        assert!(
            text.contains("confirm"),
            "Preview with draft must show 'confirm', got: {text:?}"
        );
    }

    #[test]
    fn generate_footer_hints_no_p_prompt_anywhere() {
        // The literal "p prompt" label must not appear in any hint combination.
        for focus in [Focus::Menu, Focus::Form, Focus::Preview] {
            for has_draft in [false, true] {
                let text = line_text(&generate_footer_hints(focus, has_draft));
                assert!(
                    !text.contains("prompt"),
                    "Hint 'prompt' must not appear (focus={focus:?}, has_draft={has_draft}): {text:?}"
                );
            }
        }
    }
}
