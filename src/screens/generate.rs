#[path = "generate/form.rs"]
pub mod form;
#[path = "generate/input.rs"]
mod input;

use std::cell::Cell;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use crate::domain::{
    ContextBundle, ExecuteStep, GeneratedDraft, JjOp, JjOpKind, PromptBuild, RevsetSummary,
    Revsets, StatusStore,
};
use crate::runtime::Cached;

pub use self::form::{FieldId, FieldKind, FieldState, InputMode, PrForm};
use super::Transition;
use super::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Menu,
    Form,
    Preview,
}

impl Pane {
    pub fn label(self) -> &'static str {
        match self {
            Pane::Menu => "changes",
            Pane::Form => "form",
            Pane::Preview => "preview",
        }
    }

    pub fn next(self) -> Pane {
        match self {
            Pane::Menu => Pane::Form,
            Pane::Form => Pane::Preview,
            Pane::Preview => Pane::Menu,
        }
    }

    pub fn prev(self) -> Pane {
        match self {
            Pane::Menu => Pane::Preview,
            Pane::Form => Pane::Menu,
            Pane::Preview => Pane::Form,
        }
    }
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
    Confirming {
        draft: GeneratedDraft,
        prompt: PromptBuild,
        commands: CommandPreview,
    },
    Executing {
        draft: GeneratedDraft,
    },
    JjMutating {
        op: JjOpKind,
        summary: String,
    },
    Done {
        url: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingJjOp {
    pub op: JjOp,
    pub change: String,
    pub target: String,
}

impl PendingJjOp {
    fn title(&self) -> &'static str {
        self.op.kind.label()
    }

    fn question(&self) -> String {
        match self.op.kind {
            JjOpKind::SquashWithBelow => {
                format!("Squash {} into {}?", self.change, self.target)
            }
            JjOpKind::MoveUp => format!("Move {} above {}?", self.change, self.target),
            JjOpKind::MoveDown => format!("Move {} below {}?", self.change, self.target),
        }
    }

    pub(crate) fn summary(&self) -> String {
        match self.op.kind {
            JjOpKind::SquashWithBelow => {
                format!("squashing {} into {}", self.change, self.target)
            }
            JjOpKind::MoveUp => format!("moving {} above {}", self.change, self.target),
            JjOpKind::MoveDown => format!("moving {} below {}", self.change, self.target),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjOpDialog {
    Confirm(PendingJjOp),
    Error { title: String, message: String },
}

#[derive(Debug, Default, Clone)]
pub struct CommandPreview {
    pub bookmark: String,
    pub push: String,
    pub create: String,
}

impl CommandPreview {
    fn from_form(form: &PrForm) -> Self {
        let mut create = vec![
            "tea".to_string(),
            "pr".to_string(),
            "create".to_string(),
            "--base".to_string(),
            quote_arg(form.base()),
            "--head".to_string(),
            quote_arg(form.branch()),
            "--title".to_string(),
            quote_arg(form.title()),
            "--description".to_string(),
            "<description>".to_string(),
        ];
        let labels = form.labels();
        if !labels.is_empty() {
            create.push("--labels".to_string());
            create.push(quote_arg(&labels.join(",")));
        }
        let assignees = form.assignees();
        if !assignees.is_empty() {
            create.push("--assignees".to_string());
            create.push(quote_arg(&assignees.join(",")));
        }
        if !form.milestone().is_empty() {
            create.push("--milestone".to_string());
            create.push(quote_arg(form.milestone()));
        }
        Self {
            bookmark: format!(
                "jj --no-pager bookmark set --allow-backwards {} -r {}",
                quote_arg(form.branch()),
                quote_arg(form.head())
            ),
            push: format!(
                "jj --no-pager git push --bookmark {}",
                quote_arg(form.branch())
            ),
            create: create.join(" "),
        }
    }
}

fn quote_arg(value: &str) -> String {
    if value.is_empty() {
        "\"\"".to_string()
    } else if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.' | '@' | ':'))
    {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('"', "\\\""))
    }
}

#[derive(Debug, Default)]
pub struct GenerateState {
    pub pane: Pane,
    pub revset_selected: usize,
    /// Scroll offsets for Changes and Form panes. Updated at render time so the
    /// focused item stays in view (scrolls only at the edges, not always
    /// jumping to top). Cell allows mutation through a shared reference.
    pub scroll_menu: Cell<u16>,
    pub scroll_form: Cell<u16>,
    pub scroll_preview: u16,
    pub input_mode: InputMode,
    pub field_focus: FieldId,
    pub form: PrForm,
    pub phase: GeneratePhase,
    pub jj_op_dialog: Option<JjOpDialog>,
    pub last_action: Option<&'static str>,
}

impl GenerateState {
    pub fn new(default_base: String) -> Self {
        Self {
            pane: Pane::Menu,
            revset_selected: 0,
            scroll_menu: Cell::new(0),
            scroll_form: Cell::new(0),
            scroll_preview: 0,
            input_mode: InputMode::Normal,
            field_focus: FieldId::Head,
            form: PrForm::new(default_base),
            phase: GeneratePhase::Idle,
            jj_op_dialog: None,
            last_action: None,
        }
    }

    pub fn is_in_progress(&self) -> bool {
        matches!(
            self.phase,
            GeneratePhase::Collecting
                | GeneratePhase::Generating { .. }
                | GeneratePhase::Executing { .. }
                | GeneratePhase::JjMutating { .. }
        )
    }

    pub fn done_url(&self) -> Option<&str> {
        match &self.phase {
            GeneratePhase::Done { url } => Some(url.as_str()),
            _ => None,
        }
    }

    pub fn ensure_field_options_synced(&mut self, status: &StatusStore) {
        self.form.sync_options(status);
    }

    pub fn begin_confirmation(&mut self) {
        let phase = std::mem::replace(&mut self.phase, GeneratePhase::Idle);
        self.phase = match phase {
            GeneratePhase::DraftReady { draft, prompt } => GeneratePhase::Confirming {
                draft,
                prompt,
                commands: CommandPreview::from_form(&self.form),
            },
            other => other,
        };
    }

    pub fn cancel_confirmation(&mut self) {
        let phase = std::mem::replace(&mut self.phase, GeneratePhase::Idle);
        self.phase = match phase {
            GeneratePhase::Confirming { draft, prompt, .. } => {
                GeneratePhase::DraftReady { draft, prompt }
            }
            other => other,
        };
    }

    pub fn take_confirmed_jj_op(&mut self, op: &JjOp) -> Option<PendingJjOp> {
        let dialog = self.jj_op_dialog.take()?;
        match dialog {
            JjOpDialog::Confirm(pending) if pending.op == *op => Some(pending),
            other => {
                self.jj_op_dialog = Some(other);
                None
            }
        }
    }

    pub fn show_jj_error(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.jj_op_dialog = Some(JjOpDialog::Error {
            title: title.into(),
            message: message.into(),
        });
    }

    pub fn reset_after_jj_mutation(&mut self, default_base: String, status: &StatusStore) {
        let selected = self.revset_selected;
        self.input_mode = InputMode::Normal;
        self.field_focus = FieldId::Head;
        self.form = PrForm::new(default_base);
        self.phase = GeneratePhase::Idle;
        self.jj_op_dialog = None;
        self.revset_selected = selected;
        self.ensure_field_options_synced(status);
        update_head_from_selection(self, status);
    }
}

pub fn on_key(state: &mut GenerateState, status: &StatusStore, key: KeyEvent) -> Transition {
    input::on_key(state, status, key)
}

pub(super) fn current_revset_count(status: &StatusStore) -> usize {
    match status.revsets.value() {
        Some(Revsets::Loaded(items)) => items.len(),
        _ => 0,
    }
}

pub fn update_head_from_selection(state: &mut GenerateState, status: &StatusStore) {
    if let Some(Revsets::Loaded(items)) = status.revsets.value()
        && let Some(item) = items.get(state.revset_selected)
    {
        state.form.head.set_value(item.change_id.clone());
        // A change already carrying a bookmark names its own branch; prefill it
        // so the form reflects the existing branch rather than inventing one.
        // Record it as an edit so the user can undo/redo back to the prefill.
        if let Some(bookmark) = item.bookmarks.first()
            && !bookmark.is_empty()
        {
            let bookmark = bookmark.clone();
            state.form.edit(|form| form.branch_name.set_value(bookmark));
        }
    }
}

pub(super) fn open_jj_op_dialog(
    state: &mut GenerateState,
    status: &StatusStore,
    kind: JjOpKind,
) -> Transition {
    let Some(Revsets::Loaded(items)) = status.revsets.value() else {
        state.show_jj_error("jj operation unavailable", "Changes are not loaded yet.");
        return Transition::Dirty;
    };
    let Some(change) = items.get(state.revset_selected) else {
        state.show_jj_error("jj operation unavailable", "No change is selected.");
        return Transition::Dirty;
    };
    let target_index = match kind {
        JjOpKind::MoveUp => match state.revset_selected.checked_sub(1) {
            Some(i) => i,
            None => {
                state.show_jj_error("cannot move up", "There is no change above this row.");
                return Transition::Dirty;
            }
        },
        JjOpKind::SquashWithBelow | JjOpKind::MoveDown => {
            let i = state.revset_selected + 1;
            if i >= items.len() {
                state.show_jj_error("cannot use row below", "There is no change below this row.");
                return Transition::Dirty;
            }
            i
        }
    };
    let target = &items[target_index];
    state.jj_op_dialog = Some(JjOpDialog::Confirm(PendingJjOp {
        op: JjOp {
            kind,
            change_id: change.change_id.clone(),
            target_id: target.change_id.clone(),
        },
        change: revset_dialog_label(change),
        target: revset_dialog_label(target),
    }));
    Transition::Dirty
}

/// Width below which we drop the three-pane layout. Preview at < ~16
/// columns is unreadable, so below this threshold we render only the
/// Menu plus whichever of Form / Preview is currently focused.
const MIN_3PANE_WIDTH: u16 = 100;
/// Width below which we drop the Menu pane as well — Menu + Form in
/// < ~70 cols squeezes both. Below this we render just the active
/// pane (Menu/Form/Preview based on focus).
const MIN_2PANE_WIDTH: u16 = 70;

pub fn render(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let [main, help_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    if main.width >= MIN_3PANE_WIDTH {
        let [menu_area, form_area, preview_area] = Layout::horizontal([
            Constraint::Length(28),
            Constraint::Percentage(42),
            Constraint::Fill(1),
        ])
        .areas(main);
        render_menu(state, status, frame, menu_area);
        render_form(state, frame, form_area);
        render_preview(state, status, frame, preview_area);
    } else if main.width >= MIN_2PANE_WIDTH {
        let [menu_area, content_area] =
            Layout::horizontal([Constraint::Length(28), Constraint::Fill(1)]).areas(main);
        render_menu(state, status, frame, menu_area);
        // When narrow we collapse Form+Preview into a single content
        // pane. Preview is shown when it is the focused pane, otherwise
        // the form is shown. Menu focus also defaults to Form view.
        if state.pane == Pane::Preview {
            render_preview(state, status, frame, content_area);
        } else {
            render_form(state, frame, content_area);
        }
    } else {
        // Very narrow: just one pane at a time. The user navigates with
        // ←/→ to switch which is visible.
        match state.pane {
            Pane::Menu => render_menu(state, status, frame, main),
            Pane::Form => render_form(state, frame, main),
            Pane::Preview => render_preview(state, status, frame, main),
        }
    }

    render_help(state, frame, help_area);
    if state.input_mode == InputMode::Editing
        && let FieldState::Picker(picker) = state.form.field(state.field_focus)
    {
        render_picker_modal(state, picker, frame, main);
    }
    if let Some(dialog) = &state.jj_op_dialog {
        render_jj_op_dialog(dialog, frame, main);
    }
}

fn render_menu(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let block = theme::pane_block("Changes", state.pane == Pane::Menu);
    let inner = block.inner(area);
    let inner_width = inner.width.saturating_sub(1) as usize;
    frame.render_widget(block, area);
    let focused = state.pane == Pane::Menu;
    let (lines, scroll): (Vec<Line>, u16) = match status.revsets.value() {
        Some(Revsets::Loaded(items)) if !items.is_empty() => {
            let mut lines: Vec<Line> = Vec::new();
            let mut sel_start = 0u16;
            let mut sel_end = 0u16;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    lines.push(separator_line(inner_width));
                }
                let selected = i == state.revset_selected;
                let row_start = lines.len() as u16;
                let row = revset_row_lines(item, selected, focused, inner_width);
                let row_end = row_start + row.len() as u16;
                if selected {
                    sel_start = row_start;
                    sel_end = row_end.saturating_sub(1);
                }
                lines.extend(row);
            }
            // Only scroll when the selected item hits the viewport edge.
            let visible = inner.height;
            let cur = state.scroll_menu.get();
            let scroll = if sel_start < cur {
                sel_start
            } else if sel_end + 1 > cur + visible {
                (sel_end + 1).saturating_sub(visible)
            } else {
                cur
            };
            state.scroll_menu.set(scroll);
            (lines, scroll)
        }
        Some(Revsets::Loaded(_)) => (vec![placeholder_line("no changes above trunk()")], 0),
        Some(Revsets::Errored { message }) => {
            (vec![placeholder_line(&format!("error: {message}"))], 0)
        }
        None => match &status.revsets {
            Cached::Unknown => (vec![placeholder_line("revsets not yet discovered")], 0),
            Cached::Loading => (vec![placeholder_line("loading…")], 0),
            Cached::Stale { .. } | Cached::Ready(_) => (Vec::new(), 0),
        },
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn revset_row_lines(
    item: &RevsetSummary,
    selected: bool,
    focused: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let marker = theme::selection_marker(selected);
    let style = if selected {
        theme::selected(focused)
    } else {
        theme::text()
    };

    let primary_lines = wrap_chars(&revset_primary(item), width.saturating_sub(2));
    let mut lines = vec![Line::from(vec![
        Span::styled(marker, theme::muted()),
        Span::styled(primary_lines.first().cloned().unwrap_or_default(), style),
    ])];
    for line in primary_lines.into_iter().skip(1) {
        lines.push(Line::from(Span::styled(format!("  {line}"), style)));
    }

    // Secondary description line when the row's headline is the bookmark.
    if !item.bookmarks.is_empty() && is_meaningful_description(&item.description) {
        lines.extend(
            wrap_chars(&item.description, width.saturating_sub(4))
                .into_iter()
                .map(|line| Line::from(Span::styled(format!("    {line}"), theme::muted()))),
        );
    }

    if item.commit_count > 1 {
        let summary = format!("{}c", item.commit_count);
        let (truncated, _) = truncate_ellipsis(&summary, width.saturating_sub(4));
        lines.push(Line::from(Span::styled(
            format!("    {truncated}"),
            theme::muted(),
        )));
    }

    lines
}

fn render_form(state: &GenerateState, frame: &mut Frame, area: Rect) {
    let block = theme::pane_block("PR Form", state.pane == Pane::Form);
    let inner = block.inner(area);
    let w = inner.width as usize;
    frame.render_widget(block, area);

    let scroll = form_scroll(state, w, inner.height);
    let mut cy = 0u16;

    for (index, id) in FieldId::ALL.into_iter().enumerate() {
        if index > 0 {
            form_line(frame, inner, cy, scroll, separator_line(w));
            cy += 1;
        }

        let focused = state.pane == Pane::Form && state.field_focus == id;
        let editing = focused && state.input_mode == InputMode::Editing;
        let marker = if focused { "▶ " } else { "  " };
        let style = if focused {
            theme::selected(true)
        } else {
            theme::muted()
        };
        let label = field_label(id);

        match state.form.field(id) {
            FieldState::Text(t) => {
                form_line(
                    frame,
                    inner,
                    cy,
                    scroll,
                    Line::from(Span::styled(format!("{marker}{label}:"), style)),
                );
                cy += 1;

                let indent: u16 = 2;
                let value_w = w.saturating_sub(indent as usize);
                let value_h: u16 = if t.multiline {
                    multiline_value_height(t, value_w, editing)
                } else {
                    1
                };
                let sy = inner.y as i32 + cy as i32 - scroll as i32;
                if sy >= inner.y as i32 && (sy as u16) < inner.y + inner.height {
                    let vis_h = value_h.min((inner.y + inner.height).saturating_sub(sy as u16));
                    if editing {
                        let rect = Rect {
                            x: inner.x + indent,
                            y: sy as u16,
                            width: inner.width.saturating_sub(indent),
                            height: vis_h,
                        };
                        frame.render_widget(&t.editor, rect);
                    } else {
                        let rect = Rect {
                            x: inner.x,
                            y: sy as u16,
                            width: inner.width,
                            height: vis_h,
                        };
                        let lines = if t.multiline {
                            let mut v: Vec<Line> = t
                                .value
                                .lines()
                                .flat_map(|l| {
                                    wrap_chars(l, value_w).into_iter().map(|s| {
                                        Line::from(Span::styled(format!("  {s}"), theme::text()))
                                    })
                                })
                                .take(value_h as usize)
                                .collect();
                            // If the content exceeds the visible height,
                            // mark the last visible row with "…" so the
                            // user knows there's more below.
                            let total_lines: usize =
                                t.value.lines().map(|l| wrap_chars(l, value_w).len()).sum();
                            if total_lines > value_h as usize
                                && let Some(last) = v.last_mut()
                            {
                                *last = Line::from(Span::styled("  …", theme::muted()));
                            }
                            if v.is_empty() {
                                v.push(empty_value_line());
                            }
                            while v.len() < value_h as usize {
                                v.push(Line::from(""));
                            }
                            v
                        } else if t.value.is_empty() {
                            vec![empty_value_line()]
                        } else {
                            let (display, _) = truncate_ellipsis(&t.value, value_w);
                            vec![Line::from(Span::styled(
                                format!("  {display}"),
                                theme::text(),
                            ))]
                        };
                        frame.render_widget(Paragraph::new(lines), rect);
                    }
                }
                cy += value_h;

                for error in &t.errors {
                    form_line(
                        frame,
                        inner,
                        cy,
                        scroll,
                        Line::from(Span::styled(format!("  - {error}"), theme::error())),
                    );
                    cy += 1;
                }
            }
            FieldState::Picker(_) => {
                let lines = field_lines(state, id, w);
                let fh = lines.len() as u16;
                form_block(frame, inner, cy, scroll, lines);
                cy += fh;
            }
        }
    }
}

fn form_line(frame: &mut Frame, inner: Rect, cy: u16, scroll: u16, line: Line<'static>) {
    let sy = inner.y as i32 + cy as i32 - scroll as i32;
    if sy >= inner.y as i32 && (sy as u16) < inner.y + inner.height {
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: inner.x,
                y: sy as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
}

fn form_block(frame: &mut Frame, inner: Rect, cy: u16, scroll: u16, lines: Vec<Line<'static>>) {
    let fh = lines.len() as u16;
    let sy = inner.y as i32 + cy as i32 - scroll as i32;
    let vp_bot = inner.y + inner.height;
    if sy >= vp_bot as i32 || sy + fh as i32 <= inner.y as i32 {
        return;
    }
    let skip = (inner.y as i32 - sy).max(0) as u16;
    let vis_y = sy.max(inner.y as i32) as u16;
    let vis_h = fh.saturating_sub(skip).min(vp_bot - vis_y);
    if vis_h > 0 {
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((skip, 0)),
            Rect {
                x: inner.x,
                y: vis_y,
                width: inner.width,
                height: vis_h,
            },
        );
    }
}

fn form_scroll(state: &GenerateState, width: usize, visible: u16) -> u16 {
    let mut cy = 0u16;
    for (index, id) in FieldId::ALL.into_iter().enumerate() {
        if index > 0 {
            cy += 1;
        }
        let fh = form_field_height(state, id, width);
        if id == state.field_focus {
            let cur = state.scroll_form.get();
            let scroll = if cy < cur {
                cy
            } else if cy + fh > cur + visible {
                cy + fh - visible
            } else {
                cur
            };
            state.scroll_form.set(scroll);
            return scroll;
        }
        cy += fh;
    }
    0
}

fn form_field_height(state: &GenerateState, id: FieldId, width: usize) -> u16 {
    let editing = state.field_focus == id && state.input_mode == InputMode::Editing;
    match state.form.field(id) {
        FieldState::Text(t) => {
            let value_h = if t.multiline {
                multiline_value_height(t, width.saturating_sub(2), editing)
            } else {
                1
            };
            1 + value_h + t.errors.len() as u16
        }
        FieldState::Picker(_) => field_lines(state, id, width).len() as u16,
    }
}

/// Display height of a multiline text field's value box: tall enough to show all
/// the wrapped content, but never shorter than [`MULTILINE_MIN_HEIGHT`] (the box
/// keeps its familiar minimum size even when nearly empty).
fn multiline_value_height(t: &form::TextFieldState, value_w: usize, editing: bool) -> u16 {
    const MULTILINE_MIN_HEIGHT: u16 = 6;
    let content = if editing { &t.buffer } else { &t.value };
    let lines: usize = if content.is_empty() {
        1
    } else {
        content.lines().map(|l| wrap_chars(l, value_w).len()).sum()
    };
    (lines as u16).max(MULTILINE_MIN_HEIGHT)
}

fn render_picker_modal(
    state: &GenerateState,
    picker: &form::PickerFieldState,
    frame: &mut Frame,
    area: Rect,
) {
    frame.render_widget(theme::backdrop(), area);
    let width = area.width.saturating_sub(16).clamp(24, 52);
    let height = area.height.saturating_sub(6).clamp(6, 14);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let id = state.field_focus;
    let block = theme::modal_block(id.label());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Filter: ", theme::muted()),
            Span::raw(if picker.filter.is_empty() {
                "-".to_string()
            } else {
                picker.filter.clone()
            }),
        ]),
        Line::from(""),
    ];
    let visible = picker.visible_options();
    if visible.is_empty() {
        lines.push(placeholder_line("no options"));
    } else {
        // Two header lines (filter + blank) consume vertical space; the rest is
        // the scrollable option list. Scroll naturally: hold the window steady
        // and only move it when the highlight crosses the top or bottom edge.
        let rows = (inner.height as usize).saturating_sub(lines.len()).max(1);
        let max_offset = visible.len().saturating_sub(rows);
        let cur = picker.scroll.get().min(max_offset);
        let offset = if picker.highlighted < cur {
            picker.highlighted
        } else if picker.highlighted >= cur + rows {
            picker.highlighted - rows + 1
        } else {
            cur
        };
        picker.scroll.set(offset);
        for (idx, option) in visible.into_iter().enumerate().skip(offset).take(rows) {
            let focused = idx == picker.highlighted;
            let warning = state.form.picker_option_warning(id, &option.value);
            let marker = if picker.multi_select {
                if picker.draft_contains(&option.value) {
                    "[x] "
                } else {
                    "[ ] "
                }
            } else if picker.draft_contains(&option.value) {
                "[•] "
            } else if focused {
                "▶ "
            } else {
                "  "
            };
            let style = if warning.is_some() {
                let style = theme::warning();
                if focused {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                }
            } else if focused {
                theme::selected(true)
            } else {
                theme::text()
            };
            let suffix = warning.map_or(String::new(), |text| format!("  ({text})"));
            lines.push(Line::from(Span::styled(
                format!("{marker}{}{suffix}", option.label),
                style,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_jj_op_dialog(dialog: &JjOpDialog, frame: &mut Frame, area: Rect) {
    frame.render_widget(theme::backdrop(), area);
    let width = area.width.saturating_sub(16).clamp(34, 72);
    let height = area.height.saturating_sub(8).clamp(8, 12);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let title = match dialog {
        JjOpDialog::Confirm(pending) => pending.title().to_string(),
        JjOpDialog::Error { title, .. } => title.clone(),
    };
    let block = theme::modal_block(title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = match dialog {
        JjOpDialog::Confirm(pending) => vec![
            Line::from(Span::styled(pending.question(), theme::text())),
            Line::from(""),
            Line::from(Span::styled(
                "This rewrites the stack. Conflicts will be probed and reverted.",
                theme::warning(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", theme::selected(true)),
                Span::styled(" confirm   ", theme::muted()),
                Span::styled("Esc", theme::selected(false)),
                Span::styled(" cancel", theme::muted()),
            ]),
        ],
        JjOpDialog::Error { message, .. } => vec![
            Line::from(Span::styled("Cannot run jj operation.", theme::error())),
            Line::from(""),
            Line::from(Span::raw(message.clone())),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter/Esc", theme::selected(true)),
                Span::styled(" close", theme::muted()),
            ]),
        ],
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_preview(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let block = theme::pane_block("Preview", state.pane == Pane::Preview);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = change_header_lines(state, status);
    // A single persistent `status:` heading replaces the old per-phase headings
    // (`failed:`, `collecting context:`, …). Each phase body leads with its own
    // status line, so the phase heading was redundant with the line below it.
    lines.extend(section_header("status"));
    lines.extend(match &state.phase {
        GeneratePhase::Idle => preview_idle_lines(state, status),
        GeneratePhase::Collecting => preview_collecting_lines(state),
        GeneratePhase::Generating { context, prompt } => preview_generating_lines(context, prompt),
        GeneratePhase::DraftReady { draft, prompt } => preview_draft_lines(state, draft, prompt),
        GeneratePhase::Confirming {
            draft,
            prompt,
            commands,
        } => preview_confirming_lines(state, draft, prompt, commands, inner.width),
        GeneratePhase::Executing { draft } => preview_executing_lines(draft),
        GeneratePhase::JjMutating { op, summary } => preview_jj_mutating_lines(*op, summary),
        GeneratePhase::Done { url } => preview_done_lines(url),
        GeneratePhase::Failed { message } => preview_failed_lines(message),
    });
    lines.extend(next_step_lines(state, status));
    if let Some(hint) = state.last_action {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {hint}"),
            theme::success(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((state.scroll_preview, 0)),
        inner,
    );
}

/// Compact header for the preview pane — change id + bookmarks + base.
/// We deliberately do NOT repeat title/description here; those live in
/// the Form pane to the left.
fn change_header_lines(state: &GenerateState, status: &StatusStore) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(item) = selected_revset(state, status) {
        lines.push(Line::from(Span::styled(
            item.change_id.clone(),
            theme::accent().add_modifier(Modifier::BOLD),
        )));
        if !item.bookmarks.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  {}", item.bookmarks.join(", ")),
                theme::text(),
            )));
        }
    } else {
        let hint = match (&status.revsets, status.revsets.value()) {
            (Cached::Loading, _) => "discovering changes…",
            (_, Some(Revsets::Loaded(items))) if items.is_empty() => "no changes above trunk()",
            (_, Some(Revsets::Errored { message })) => message.as_str(),
            _ => "no change selected",
        };
        lines.push(Line::from(Span::styled(
            fmt_or_dash(state.form.head()),
            theme::accent(),
        )));
        lines.push(hint_line(hint));
    }
    lines.push(Line::from(""));
    lines.push(kv_line("base", fmt_or_dash(state.form.base())));
    lines
}

/// Idle preview: show context the Form does NOT show — diff stat,
/// commit count/log, last action — plus a one-line CTA.
fn preview_idle_lines(state: &GenerateState, status: &StatusStore) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(item) = selected_revset(state, status) {
        lines.push(status_line("Ready to generate."));
        if let Some(stat) = compact_diff_stat(&item.stats) {
            lines.push(kv_line("diff", stat));
        }
        if item.commit_count > 0 {
            lines.push(kv_line("commits", item.commit_count.to_string()));
        }
        if !item.author.is_empty() {
            lines.push(kv_line("author", item.author.clone()));
        }
        if !item.recent_log.is_empty() {
            lines.push(Line::from(""));
            lines.push(section_heading_line("log"));
            for entry in item.recent_log.iter().take(6) {
                lines.push(Line::from(Span::styled(
                    format!("  {entry}"),
                    theme::muted(),
                )));
            }
        }
        for warning in &item.warnings {
            lines.push(Line::from(Span::styled(
                format!("  ! {warning}"),
                theme::warning(),
            )));
        }
    } else {
        lines.push(status_line("No change selected."));
    }
    lines
}

fn preview_collecting_lines(state: &GenerateState) -> Vec<Line<'static>> {
    vec![
        status_line("Collecting context…"),
        kv_line("head", fmt_or_dash(state.form.head())),
    ]
}

fn preview_generating_lines(context: &ContextBundle, prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = vec![
        status_line("Generating draft…"),
        kv_line("base", context.base.clone()),
        kv_line("head", context.head.clone()),
        kv_line("prompt", fmt_bytes(prompt.manifest.total_bytes)),
    ];
    lines.extend(prompt_manifest_lines(prompt));
    lines
}

fn preview_draft_lines(
    state: &GenerateState,
    draft: &GeneratedDraft,
    prompt: &PromptBuild,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        status_line(&format!(
            "Generated draft for {}",
            fmt_or_dash(state.form.branch())
        )),
        hint_line("The generated draft is editable in the center pane."),
    ];
    lines.extend(draft_lines(state, draft));
    lines.extend(manifest_warning_lines(prompt));
    lines
}

fn preview_confirming_lines(
    state: &GenerateState,
    draft: &GeneratedDraft,
    prompt: &PromptBuild,
    commands: &CommandPreview,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        status_line("Ready to execute."),
        kv_line("prompt", fmt_bytes(prompt.manifest.total_bytes)),
    ];
    lines.extend(draft_summary_lines(state, draft));
    lines.extend(execution_plan_lines(commands, width));
    lines
}

fn preview_executing_lines(draft: &GeneratedDraft) -> Vec<Line<'static>> {
    let mut lines = vec![
        status_line("Executing…"),
        kv_line("title", draft.title.clone()),
    ];
    lines.extend(section_header("jobs"));
    for step in [
        ExecuteStep::Bookmark,
        ExecuteStep::Push,
        ExecuteStep::Create,
    ] {
        lines.push(Line::from(Span::styled(
            format!("  - {}", step.label()),
            theme::muted(),
        )));
    }
    lines
}

fn preview_jj_mutating_lines(op: JjOpKind, summary: &str) -> Vec<Line<'static>> {
    vec![
        status_line(&format!("Running jj {}…", op.label())),
        kv_line("operation", summary.to_string()),
        Line::from(""),
        Line::from(Span::styled(
            "  Stack rewrite in progress. Navigation stays available; actions are blocked.",
            theme::warning(),
        )),
    ]
}

fn preview_done_lines(url: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "  Execution complete.",
            theme::success().add_modifier(Modifier::BOLD),
        )),
        kv_line("pr url", url.to_string()),
    ]
}

fn preview_failed_lines(message: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "  failed",
            theme::error().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(format!("  {message}"))),
    ]
}

fn next_step_lines(state: &GenerateState, status: &StatusStore) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        section_heading_line("next step"),
        hint_line(next_step_text(state, status)),
    ]
}

fn next_step_text(state: &GenerateState, status: &StatusStore) -> &'static str {
    match &state.phase {
        GeneratePhase::Idle if selected_revset(state, status).is_none() => {
            "Select a change before generating a PR draft."
        }
        GeneratePhase::Idle if state.pane == Pane::Menu => {
            "Press Enter to choose the selected change."
        }
        GeneratePhase::Idle => "Press g to assemble context and generate a PR draft.",
        GeneratePhase::Collecting => "Wait for context collection to finish.",
        GeneratePhase::Generating { .. } => "Wait for the LLM draft to finish.",
        GeneratePhase::DraftReady { .. } if state.pane == Pane::Preview => {
            "Press x to review the commands before creating the PR."
        }
        GeneratePhase::DraftReady { .. } => "Switch to the preview pane to review commands.",
        GeneratePhase::Confirming { .. } if state.pane == Pane::Preview => {
            "Press x or Enter to create the PR."
        }
        GeneratePhase::Confirming { .. } => "Switch to the preview pane to create the PR.",
        GeneratePhase::Executing { .. } => "Wait for the PR creation commands to finish.",
        GeneratePhase::JjMutating { .. } => "Wait for the jj operation to finish.",
        GeneratePhase::Done { .. } => "Press o to open the created PR, or c to copy its URL.",
        GeneratePhase::Failed { .. } => "Fix the issue, then press g to retry generation.",
    }
}

fn fmt_or_dash(s: &str) -> String {
    if s.is_empty() {
        "-".to_string()
    } else {
        s.to_string()
    }
}

fn placeholder_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), theme::muted()))
}

/// Empty-textarea placeholder, indented to align with normal value lines.
/// Reused everywhere a text field shows no content so single-line and
/// multiline fields read identically when empty.
fn empty_value_line() -> Line<'static> {
    Line::from(Span::styled("  (empty)", theme::muted()))
}

fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {text}"), theme::muted()))
}

fn kv_line(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key}: "), theme::muted()),
        Span::styled(value, theme::text()),
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

fn field_lines(state: &GenerateState, id: FieldId, width: usize) -> Vec<Line<'static>> {
    let field = state.form.field(id);
    let focused = state.pane == Pane::Form && state.field_focus == id;
    let editing = focused && state.input_mode == InputMode::Editing;
    let marker = if focused { "▶ " } else { "  " };
    let style = if focused {
        theme::selected(true)
    } else {
        theme::muted()
    };
    let label = field_label(id);
    let mut lines = Vec::new();

    match field {
        FieldState::Text(t) if t.multiline => {
            lines.push(field_header_line(marker, label, style));
            let value = if editing { &t.buffer } else { &t.value };
            let preview: Vec<&str> = value.lines().take(6).collect();
            if preview.is_empty() || value.is_empty() {
                lines.push(Line::from(Span::styled("  (empty)", theme::muted())));
            } else {
                for line in preview {
                    for wrapped in wrap_chars(line, width.saturating_sub(2)) {
                        lines.push(Line::from(Span::styled(
                            format!("  {wrapped}"),
                            theme::text(),
                        )));
                    }
                }
                let remaining = value.lines().count().saturating_sub(6);
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("  ... {remaining} more lines"),
                        theme::muted(),
                    )));
                }
            }
        }
        FieldState::Text(t) => {
            let value = if editing { &t.buffer } else { &t.value };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{marker}{label}: "),
                    style.add_modifier(if focused {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(fmt_or_dash(value), theme::text()),
            ]));
        }
        FieldState::Picker(p) => {
            lines.push(field_header_line(marker, label, style));
            let selected = if p.value.is_empty() {
                "(none)".to_string()
            } else {
                let (truncated, _) = truncate_ellipsis(&p.value, width.saturating_sub(2));
                truncated
            };
            lines.push(Line::from(Span::styled(
                format!("  {selected}"),
                theme::text(),
            )));
            if let Some(warning) = state.form.relative_order_warning(id) {
                lines.push(Line::from(Span::styled(
                    format!("  ! {warning}"),
                    theme::warning(),
                )));
            }
            if editing {
                lines.push(Line::from(Span::styled("  (editing…)", theme::muted())));
            } else if p.options.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (no options loaded)",
                    theme::muted(),
                )));
            }
        }
    }

    for error in field.errors() {
        lines.push(Line::from(Span::styled(
            format!("  - {error}"),
            theme::error(),
        )));
    }
    lines
}

fn field_label(id: FieldId) -> &'static str {
    match id {
        FieldId::BranchName => "branch name",
        _ => id.label(),
    }
}

fn field_header_line(marker: &str, label: &str, style: Style) -> Line<'static> {
    Line::from(Span::styled(format!("{marker}{label}:"), style))
}

fn selected_revset<'a>(
    state: &GenerateState,
    status: &'a StatusStore,
) -> Option<&'a RevsetSummary> {
    let Some(Revsets::Loaded(items)) = status.revsets.value() else {
        return None;
    };
    items.get(state.revset_selected)
}

fn status_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {text}"),
        theme::accent().add_modifier(Modifier::BOLD),
    ))
}

fn section_header(title: &str) -> Vec<Line<'static>> {
    vec![Line::from(""), section_heading_line(title)]
}

fn section_heading_line(title: &str) -> Line<'static> {
    theme::header(format!("{title}:"))
}

fn draft_lines(state: &GenerateState, draft: &GeneratedDraft) -> Vec<Line<'static>> {
    let mut lines = section_header("draft");
    lines.push(kv_line("branch", fmt_or_dash(state.form.branch())));
    lines.push(kv_line("title", draft.title.clone()));
    lines.push(kv_line("body chars", draft.description.len().to_string()));
    lines.push(Line::from(""));
    lines.push(section_heading_line("body"));
    if draft.description.is_empty() {
        lines.push(Line::from(Span::styled("  (empty)", theme::muted())));
    } else {
        for line in draft.description.lines() {
            lines.push(Line::from(Span::raw(format!("  {line}"))));
        }
    }
    lines
}

fn draft_summary_lines(state: &GenerateState, draft: &GeneratedDraft) -> Vec<Line<'static>> {
    vec![
        kv_line("branch", fmt_or_dash(state.form.branch())),
        kv_line("title", draft.title.clone()),
        kv_line("body chars", draft.description.len().to_string()),
    ]
}

fn prompt_manifest_lines(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = section_header("prompt manifest");
    lines.push(kv_line(
        "prompt bytes",
        fmt_bytes(prompt.manifest.total_bytes),
    ));
    lines.push(kv_line(
        "sections",
        prompt.manifest.sections.len().to_string(),
    ));
    lines.extend(section_header("included sections"));
    if prompt.manifest.sections.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", theme::muted())));
    } else {
        for section in &prompt.manifest.sections {
            lines.push(Line::from(Span::styled(
                format!("  - {} ({})", section.name, fmt_bytes(section.bytes)),
                theme::text(),
            )));
        }
    }
    lines
}

fn manifest_warning_lines(prompt: &PromptBuild) -> Vec<Line<'static>> {
    let mut lines = section_header("manifest warnings");
    if prompt.manifest.sections.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no prompt sections were included",
            theme::warning(),
        )));
    } else {
        lines.push(Line::from(Span::styled("  (none)", theme::muted())));
    }
    lines
}

fn execution_plan_lines(commands: &CommandPreview, width: u16) -> Vec<Line<'static>> {
    let mut lines = section_header("execution plan");
    // Reserve 5 cols for the "     " indent; truncate to width with "…"
    // rather than wrap — wrapped shell commands are unsafe to copy.
    let cmd_width = (width as usize).saturating_sub(5);
    for (index, (label, command)) in [
        ("bookmark", commands.bookmark.as_str()),
        ("push", commands.push.as_str()),
        ("create PR", commands.create.as_str()),
    ]
    .into_iter()
    .enumerate()
    {
        lines.push(Line::from(Span::styled(
            format!("  {}. {label}", index + 1),
            theme::muted().add_modifier(Modifier::BOLD),
        )));
        let (truncated, cut) = truncate_ellipsis(command, cmd_width);
        lines.push(Line::from(Span::styled(
            format!("     {truncated}"),
            theme::text(),
        )));
        if cut {
            lines.push(Line::from(Span::styled(
                "     (command truncated to fit pane)",
                theme::muted(),
            )));
        }
    }
    lines
}

/// Truncate to at most `width` display chars, suffixing with "…" if cut.
/// Returns `(string, was_truncated)`.
fn truncate_ellipsis(value: &str, width: usize) -> (String, bool) {
    if width == 0 {
        return (String::new(), !value.is_empty());
    }
    let count = value.chars().count();
    if count <= width {
        return (value.to_string(), false);
    }
    let take = width.saturating_sub(1);
    let mut out: String = value.chars().take(take).collect();
    out.push('…');
    (out, true)
}

pub(super) fn revset_primary(item: &RevsetSummary) -> String {
    if let Some(bookmark) = item.bookmarks.first()
        && !bookmark.is_empty()
    {
        return bookmark.clone();
    }
    if is_meaningful_description(&item.description) {
        return item.description.clone();
    }
    if !item.change_id.is_empty() {
        return item.change_id.clone();
    }
    fmt_or_dash(&item.label)
}

fn revset_dialog_label(item: &RevsetSummary) -> String {
    let primary = revset_primary(item);
    if primary == item.change_id {
        primary
    } else {
        format!("{} {}", item.change_id, primary)
    }
}

fn compact_diff_stat(stats: &str) -> Option<String> {
    stats
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn is_meaningful_description(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed != "(no description set)"
        && trimmed != "No description set."
        && trimmed != "no description set"
}

fn separator_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}  ", "─".repeat(width.saturating_sub(4))),
        Style::default().fg(theme::BORDER),
    ))
}

fn wrap_chars(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let current_len = current.chars().count();
        let word_len = word.chars().count();
        if current_len == 0 {
            current.push_str(word);
        } else if current_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn render_help(state: &GenerateState, frame: &mut Frame, area: Rect) {
    use theme::HelpHint;
    let hints: Vec<HelpHint> = if state.input_mode == InputMode::Editing {
        match state.form.field(state.field_focus).kind() {
            FieldKind::Text { multiline: true } => vec![
                HelpHint::primary("Esc", "commit"),
                HelpHint::new("Enter", "newline"),
            ],
            FieldKind::Text { multiline: false } => vec![
                HelpHint::primary("Enter", "commit"),
                HelpHint::new("Esc", "commit"),
            ],
            FieldKind::Picker {
                multi_select: true, ..
            } => vec![
                HelpHint::new("Space", "toggle"),
                HelpHint::primary("Enter", "commit"),
                HelpHint::new("Esc", "cancel"),
            ],
            FieldKind::Picker { .. } => vec![
                HelpHint::primary("Enter", "commit"),
                HelpHint::new("Esc", "cancel"),
            ],
        }
    } else if matches!(
        state.phase,
        GeneratePhase::Collecting
            | GeneratePhase::Generating { .. }
            | GeneratePhase::JjMutating { .. }
    ) {
        match &state.phase {
            GeneratePhase::JjMutating { .. } => vec![HelpHint::primary("Wait", "jj running")],
            _ => {
                // Generation in flight: Esc aborts it (the headline action). `b` still
                // works (it only changes the backend for the next run) and is shown
                // everywhere for consistency.
                vec![
                    HelpHint::primary("Esc", "cancel"),
                    HelpHint::new("b", "backend"),
                ]
            }
        }
    } else {
        normal_help_hints(state)
    };
    frame.render_widget(Paragraph::new(theme::help_line(&hints, area.width)), area);
}

/// Help hints for a non-editing, non-generating screen. Each key is listed
/// exactly where it actually does something (mirroring the gates in
/// `generate::input::on_key`): the pane-local action first, then `g` wherever a
/// draft can be (re)generated, then `b` and `Esc` — both available everywhere.
/// Navigation keys (q, arrows, hjkl) are intentionally never surfaced.
fn normal_help_hints(state: &GenerateState) -> Vec<theme::HelpHint<'static>> {
    use theme::HelpHint;
    let mut hints: Vec<HelpHint> = Vec::new();

    match (state.pane, &state.phase) {
        (Pane::Menu, _) => {
            hints.push(HelpHint::primary("Enter", "pick"));
            hints.push(HelpHint::new("r", "refresh"));
            if !state.is_in_progress() && state.jj_op_dialog.is_none() {
                hints.push(HelpHint::new("s", "squash below"));
                hints.push(HelpHint::new("J/K", "move"));
            }
        }
        (Pane::Preview, GeneratePhase::DraftReady { .. }) => {
            hints.push(HelpHint::primary("x", "review"));
        }
        (Pane::Preview, GeneratePhase::Confirming { .. }) => {
            hints.push(HelpHint::primary("Enter/x", "execute"));
        }
        (Pane::Preview, GeneratePhase::Done { .. }) => {
            hints.push(HelpHint::primary("o", "open"));
            hints.push(HelpHint::new("c", "copy URL"));
        }
        (Pane::Form, _) => hints.push(HelpHint::new("Enter/i", "edit")),
        (Pane::Preview, _) => {}
    }

    // Undo/redo live wherever the form content is shown. `u`/`r` move the whole
    // form; `U`/`R` move just the highlighted field (Form pane only).
    if matches!(state.pane, Pane::Form | Pane::Preview) {
        hints.push(HelpHint::new("u/r", "undo/redo"));
    }

    // `g` (re)generates from any pane but Menu while no job is running.
    if state.pane != Pane::Menu && !state.is_in_progress() {
        let label = if matches!(
            state.phase,
            GeneratePhase::DraftReady { .. } | GeneratePhase::Confirming { .. }
        ) {
            "regenerate"
        } else {
            "generate"
        };
        hints.push(promote_if_first(HelpHint::new("g", label), &hints));
    }

    hints.push(HelpHint::new("b", "backend"));
    // Esc leaves the screen (or steps back from confirmation); highlight it only
    // when nothing more specific has claimed the primary slot.
    hints.push(promote_if_first(HelpHint::new("Esc", "back"), &hints));
    hints
}

/// Mark `hint` as the primary (highlighted) one unless an earlier hint already
/// is, so each help line has exactly one highlight.
fn promote_if_first<'a>(
    hint: theme::HelpHint<'a>,
    existing: &[theme::HelpHint<'a>],
) -> theme::HelpHint<'a> {
    if existing.iter().any(|h| h.primary) {
        hint
    } else {
        theme::HelpHint::primary(hint.key, hint.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PromptManifest, WorkspaceInfo};

    fn status_with_revset() -> StatusStore {
        let mut status = StatusStore::new();
        status.set_workspace(WorkspaceInfo::Outside);
        status.set_revsets(Revsets::Loaded(vec![RevsetSummary {
            label: "trunk()..abcd".into(),
            change_id: "abcd".into(),
            commit_id: "deadbeef".into(),
            bookmarks: Vec::new(),
            description: "Add foo".into(),
            description_body: String::new(),
            author: String::new(),
            stats: String::new(),
            commit_count: 1,
            commit_ids: vec!["deadbeef".into()],
            change_ids: vec!["abcd".into()],
            recent_log: Vec::new(),
            warnings: Vec::new(),
        }]));
        status
    }

    fn generate_state(phase: GeneratePhase, pane: Pane) -> GenerateState {
        let mut state = GenerateState::new("main".into());
        state.form.head.set_value("abcd".into());
        state.form.branch_name.set_value("add-foo".into());
        state.form.title.set_value("Add foo".into());
        state.form.description.set_value("Body".into());
        state.phase = phase;
        state.pane = pane;
        state
    }

    #[test]
    fn next_step_for_idle_preview_generates_draft() {
        let status = status_with_revset();
        let state = generate_state(GeneratePhase::Idle, Pane::Preview);

        assert_eq!(
            next_step_text(&state, &status),
            "Press g to assemble context and generate a PR draft."
        );
    }

    #[test]
    fn next_step_for_draft_ready_preview_reviews_commands() {
        let status = status_with_revset();
        let state = generate_state(
            GeneratePhase::DraftReady {
                draft: GeneratedDraft {
                    pr_type: "feat".into(),
                    branch_slug: "add-foo".into(),
                    title: "Add foo".into(),
                    description: "Body".into(),
                },
                prompt: PromptBuild {
                    prompt: "prompt".into(),
                    manifest: PromptManifest {
                        sections: Vec::new(),
                        total_bytes: 0,
                    },
                },
            },
            Pane::Preview,
        );

        assert_eq!(
            next_step_text(&state, &status),
            "Press x to review the commands before creating the PR."
        );
    }

    #[test]
    fn next_step_for_confirmation_executes_pr() {
        let status = status_with_revset();
        let state = generate_state(
            GeneratePhase::Confirming {
                draft: GeneratedDraft {
                    pr_type: "feat".into(),
                    branch_slug: "add-foo".into(),
                    title: "Add foo".into(),
                    description: "Body".into(),
                },
                prompt: PromptBuild {
                    prompt: "prompt".into(),
                    manifest: PromptManifest {
                        sections: Vec::new(),
                        total_bytes: 0,
                    },
                },
                commands: CommandPreview::default(),
            },
            Pane::Preview,
        );

        assert_eq!(
            next_step_text(&state, &status),
            "Press x or Enter to create the PR."
        );
    }

    fn draft_ready_phase() -> GeneratePhase {
        GeneratePhase::DraftReady {
            draft: GeneratedDraft {
                pr_type: "feat".into(),
                branch_slug: "add-foo".into(),
                title: "Add foo".into(),
                description: "Body".into(),
            },
            prompt: PromptBuild {
                prompt: "prompt".into(),
                manifest: PromptManifest {
                    sections: Vec::new(),
                    total_bytes: 0,
                },
            },
        }
    }

    fn help_keys(phase: GeneratePhase, pane: Pane) -> Vec<&'static str> {
        normal_help_hints(&generate_state(phase, pane))
            .into_iter()
            .map(|hint| hint.key)
            .collect()
    }

    #[test]
    fn backend_is_offered_in_every_pane() {
        for pane in [Pane::Menu, Pane::Form, Pane::Preview] {
            assert!(
                help_keys(GeneratePhase::Idle, pane).contains(&"b"),
                "backend hint missing in {pane:?}"
            );
        }
    }

    #[test]
    fn review_is_only_offered_where_it_works() {
        // `x` review fires only from the Preview pane, so the Form pane must not
        // advertise it even though the draft is ready.
        assert!(help_keys(draft_ready_phase(), Pane::Preview).contains(&"x"));
        assert!(!help_keys(draft_ready_phase(), Pane::Form).contains(&"x"));
    }

    #[test]
    fn generate_is_offered_wherever_it_works() {
        // Anywhere but the Menu pane, with no job running, `g` is available —
        // including the Preview pane while idle, which previously omitted it.
        assert!(help_keys(GeneratePhase::Idle, Pane::Preview).contains(&"g"));
        assert!(help_keys(GeneratePhase::Idle, Pane::Form).contains(&"g"));
        assert!(!help_keys(GeneratePhase::Idle, Pane::Menu).contains(&"g"));
    }

    #[test]
    fn exactly_one_hint_is_primary() {
        for pane in [Pane::Menu, Pane::Form, Pane::Preview] {
            for phase in [GeneratePhase::Idle, draft_ready_phase()] {
                let state = generate_state(phase, pane);
                let primaries = normal_help_hints(&state)
                    .into_iter()
                    .filter(|hint| hint.primary)
                    .count();
                assert_eq!(primaries, 1, "expected one primary hint in {pane:?}");
            }
        }
    }
}
