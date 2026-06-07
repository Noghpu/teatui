#[path = "generate/form.rs"]
pub mod form;
#[path = "generate/input.rs"]
mod input;

use std::cell::Cell;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::config::Config;
use crate::domain::{
    BulkPhase, ContextBundle, CreatePrInput, ExecuteStep, ForgeCli, GeneratedDraft, JjOp, JjOpKind,
    LlmHealth, PromptBuild, RevsetSummary, Revsets, StackPlanItem, StatusStore, ToolStatus,
    WorkspaceInfo,
};
use crate::runtime::Cached;

pub use self::form::{FieldId, FieldKind, FieldState, InputMode, PrForm};
use super::widgets::{
    field_header_line, form_block, form_line, hint_line, kv_line, kv_line_fit,
    multiline_value_height, placeholder_line, render_text_field, section_header,
    section_heading_line, separator_line, status_line, wrapped_styled_lines,
};
use super::{Transition, theme, util};

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
    fn from_form(form: &PrForm, forge: &ForgeCli) -> Self {
        let labels = form.labels();
        let assignees = form.assignees();
        let input = CreatePrInput {
            base: form.base(),
            head: form.branch(),
            title: form.title(),
            description: "<description>",
            labels: &labels,
            assignees: &assignees,
            milestone: form.milestone(),
        };
        let create = std::iter::once(forge.binary().to_string())
            .chain(
                forge
                    .create_args(&input)
                    .into_iter()
                    .map(|arg| quote_arg(&arg)),
            )
            .collect::<Vec<_>>();
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

/// Per-PR editing state inside the bulk review modal.
/// Reuses three `TextFieldState` for title, branch, and description.
#[derive(Debug, Clone)]
pub struct BulkItemEditor {
    pub title: form::TextFieldState,
    pub branch: form::TextFieldState,
    pub description: form::TextFieldState,
    /// Which field is focused in the per-PR form.
    pub field_focus: BulkItemField,
    /// True when the user is actively editing a field.
    pub editing: bool,
}

impl Default for BulkItemEditor {
    fn default() -> Self {
        Self {
            title: form::TextFieldState::new(String::new(), false),
            branch: form::TextFieldState::new(String::new(), false),
            description: form::TextFieldState::new(String::new(), true),
            field_focus: BulkItemField::Title,
            editing: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BulkItemField {
    #[default]
    Title,
    Branch,
    Description,
}

impl BulkItemField {
    fn next(self) -> Self {
        match self {
            BulkItemField::Title => BulkItemField::Branch,
            BulkItemField::Branch => BulkItemField::Description,
            BulkItemField::Description => BulkItemField::Description,
        }
    }

    fn prev(self) -> Self {
        match self {
            BulkItemField::Title => BulkItemField::Title,
            BulkItemField::Branch => BulkItemField::Title,
            BulkItemField::Description => BulkItemField::Branch,
        }
    }

    fn label(self) -> &'static str {
        match self {
            BulkItemField::Title => "title",
            BulkItemField::Branch => "branch",
            BulkItemField::Description => "description",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BulkReviewFocus {
    #[default]
    List,
    /// The editable form pane (Title / Branch / Description). Named "Preview"
    /// internally for historical compatibility; the user-visible pane title is
    /// "Selected PR".
    Preview,
    /// The messages/status pane (blockers, warnings, push results, last action).
    Messages,
}

impl BulkItemEditor {
    pub fn from_plan_item(item: &StackPlanItem) -> Self {
        Self {
            title: form::TextFieldState::new(item.title.clone(), false),
            branch: form::TextFieldState::new(item.bookmark.clone(), false),
            description: form::TextFieldState::new(item.description.clone(), true),
            field_focus: BulkItemField::Title,
            editing: false,
        }
    }

    pub fn commit_to_plan_item(&self, item: &mut StackPlanItem) {
        item.title = self.title.value.clone();
        item.bookmark = self.branch.value.clone();
        item.description = self.description.value.clone();
    }

    fn field(&self, f: BulkItemField) -> &form::TextFieldState {
        match f {
            BulkItemField::Title => &self.title,
            BulkItemField::Branch => &self.branch,
            BulkItemField::Description => &self.description,
        }
    }

    fn field_mut(&mut self, f: BulkItemField) -> &mut form::TextFieldState {
        match f {
            BulkItemField::Title => &mut self.title,
            BulkItemField::Branch => &mut self.branch,
            BulkItemField::Description => &mut self.description,
        }
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
    pub last_action: Option<String>,
    /// Selected PR heads, stored by `change_id` in the order they were toggled.
    /// The stored order is not load-bearing: consumers re-derive a stable
    /// oldest-to-newest order from the revsets list (see `selected_heads_present`
    /// and `derive_stack_ranges`). Stale ids (no longer in
    /// `StatusStore::revsets`) are dropped on read.
    pub selected_heads: Vec<String>,
    /// Phase of the bulk stacked-PR flow. Only `Idle` is active in slice 1.
    pub bulk: BulkPhase,
    /// Active side of the bulk review modal. Selecting a list row moves focus
    /// to Preview; editing only starts from Preview focus.
    pub bulk_review_focus: BulkReviewFocus,
    /// Per-PR editor state for the bulk review modal. Seeded from the
    /// highlighted `StackPlanItem` when entering `Review`; kept in sync as
    /// the cursor moves.
    pub bulk_editor: BulkItemEditor,
    /// Scroll offset for the PR list in the bulk review modal.
    pub bulk_list_scroll: Cell<u16>,
    /// Scroll offset for the right (Selected PR) pane in the bulk review modal.
    /// Updated at render time using the natural-scroll pattern so the focused
    /// field stays fully visible. Cell allows mutation through a shared reference.
    pub bulk_form_scroll: Cell<u16>,
    /// Scroll offset for the Messages pane in the bulk review modal.
    /// Controlled by Up/Down/j/k when the Messages pane has focus.
    pub bulk_messages_scroll: Cell<usize>,
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
            selected_heads: Vec::new(),
            bulk: BulkPhase::Idle,
            bulk_review_focus: BulkReviewFocus::List,
            bulk_editor: BulkItemEditor::default(),
            bulk_list_scroll: Cell::new(0),
            bulk_form_scroll: Cell::new(0),
            bulk_messages_scroll: Cell::new(0),
        }
    }

    /// Toggle `change_id` in/out of the selected-head set.
    /// If already present it is removed; otherwise it is appended. The stored
    /// order is toggle order; display and range-derivation order are recomputed
    /// from the revsets list on read, so a plain append is sufficient here.
    pub fn toggle_selected_head(&mut self, change_id: &str) {
        if let Some(pos) = self.selected_heads.iter().position(|id| id == change_id) {
            self.selected_heads.remove(pos);
        } else {
            self.selected_heads.push(change_id.to_string());
        }
    }

    /// Return `true` when `change_id` is currently in the selected-head set.
    pub fn is_head_selected(&self, change_id: &str) -> bool {
        self.selected_heads.iter().any(|id| id == change_id)
    }

    /// Collect selected heads that are still present in the loaded revsets,
    /// preserving their display order (newest-first) from the revsets list.
    /// This also serves as the stale-id filter: ids absent from `revsets` are
    /// silently dropped.
    pub fn selected_heads_present<'a>(
        &self,
        revsets: &'a [RevsetSummary],
    ) -> Vec<&'a RevsetSummary> {
        revsets
            .iter()
            .filter(|r| self.is_head_selected(&r.change_id))
            .collect()
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

    /// True when any job is in flight: single-PR scalar jobs, bulk stack jobs
    /// (`Collecting` / `Generating` / `Review { pushing: Some(_) }`), or Tier D
    /// `JjMutating`. Input gates and Tier D keys use this to stay consistent.
    pub fn has_busy_job(&self) -> bool {
        if self.is_in_progress() {
            return true;
        }
        matches!(
            &self.bulk,
            BulkPhase::Collecting
                | BulkPhase::Generating { .. }
                | BulkPhase::Review {
                    pushing: Some(_),
                    ..
                }
        )
    }

    /// Derive the oldest selected head from the revsets list, for the Form's
    /// derived read-only `head` in bulk mode. Returns `None` when there are no
    /// selected heads or no loaded revsets.
    pub fn bulk_derived_head<'a>(&self, revsets: &'a [RevsetSummary]) -> Option<&'a RevsetSummary> {
        // `selected_heads_present` returns in newest-first display order;
        // the oldest is the last.
        self.selected_heads_present(revsets).into_iter().last()
    }

    /// Seed the `bulk_editor` from the item at `cursor` in the plan.
    pub fn seed_bulk_editor_from_cursor(&mut self) {
        if let BulkPhase::Review { plan, cursor, .. } = &self.bulk
            && let Some(item) = plan.items.get(*cursor)
        {
            self.bulk_editor = BulkItemEditor::from_plan_item(item);
        }
    }

    /// Flush the current `bulk_editor` values back into the plan at `cursor`.
    pub fn flush_bulk_editor_to_plan(&mut self) {
        if let BulkPhase::Review { plan, cursor, .. } = &mut self.bulk {
            let idx = *cursor;
            if let Some(item) = plan.items.get_mut(idx) {
                self.bulk_editor.commit_to_plan_item(item);
            }
        }
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

    pub fn begin_confirmation(&mut self, forge: &ForgeCli) {
        let phase = std::mem::replace(&mut self.phase, GeneratePhase::Idle);
        self.phase = match phase {
            GeneratePhase::DraftReady { draft, prompt } => GeneratePhase::Confirming {
                draft,
                prompt,
                commands: CommandPreview::from_form(&self.form, forge),
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

pub fn render(
    state: &GenerateState,
    status: &StatusStore,
    config: &Config,
    frame: &mut Frame,
    area: Rect,
) {
    let [main, status_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

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

    render_generate_status_bar(state, status, config, frame, status_area);
    render_help(state, frame, help_area);
    if state.input_mode == InputMode::Editing
        && let FieldState::Picker(picker) = state.form.field(state.field_focus)
    {
        render_picker_modal(state, picker, frame, main);
    }
    if let Some(dialog) = &state.jj_op_dialog {
        render_jj_op_dialog(dialog, frame, main);
    }
    // The bulk modal is drawn last so it sits on top of all other content.
    if !matches!(state.bulk, BulkPhase::Idle) {
        render_bulk_modal(state, frame, main);
    }
}

fn render_generate_status_bar(
    state: &GenerateState,
    status: &StatusStore,
    config: &Config,
    frame: &mut Frame,
    area: Rect,
) {
    let mode = match state.input_mode {
        InputMode::Normal => "Normal",
        InputMode::Editing => "Editing",
    };
    let chips = vec![
        theme::StatusChip::mode(mode),
        theme::StatusChip::plain("Generate", 3),
        theme::StatusChip::styled(forge_status_label(status), 2, forge_status_style(status)),
        theme::StatusChip::styled(
            llm_status_label(status, config),
            1,
            llm_status_style(status),
        ),
    ];
    frame.render_widget(Paragraph::new(theme::status_line(chips, area.width)), area);
}

fn forge_status_label(status: &StatusStore) -> String {
    let mut label = status.forge_label.clone();
    if let Some(host) = workspace_remote_host(status) {
        label.push_str(" · ");
        label.push_str(host);
    }
    label
}

fn workspace_remote_host(status: &StatusStore) -> Option<&str> {
    match status.workspace.value() {
        Some(WorkspaceInfo::Inside {
            remote: Some(remote),
            ..
        }) => Some(remote.host.as_str()),
        _ => None,
    }
}

fn forge_status_style(status: &StatusStore) -> ratatui::style::Style {
    match cached_tool_health(&status.forge) {
        GenerateChipHealth::Good => theme::success(),
        GenerateChipHealth::Bad => theme::error(),
        GenerateChipHealth::Pending => theme::muted(),
    }
}

fn llm_status_style(status: &StatusStore) -> ratatui::style::Style {
    match &status.llm {
        Cached::Ready(LlmHealth::Available { .. })
        | Cached::Stale {
            value: LlmHealth::Available { .. },
            ..
        } => theme::success(),
        Cached::Unknown
        | Cached::Loading
        | Cached::Ready(LlmHealth::Unreachable { .. })
        | Cached::Stale {
            value: LlmHealth::Unreachable { .. },
            ..
        } => theme::muted(),
    }
}

fn llm_status_label(status: &StatusStore, config: &Config) -> String {
    match &status.llm {
        Cached::Unknown | Cached::Loading => "LLM: pending".to_string(),
        Cached::Ready(LlmHealth::Available { .. })
        | Cached::Stale {
            value: LlmHealth::Available { .. },
            ..
        } => format!("LLM: {}", config.llm.active_backend().model),
        Cached::Ready(LlmHealth::Unreachable { .. })
        | Cached::Stale {
            value: LlmHealth::Unreachable { .. },
            ..
        } => "LLM: unreachable".to_string(),
    }
}

enum GenerateChipHealth {
    Good,
    Bad,
    Pending,
}

fn cached_tool_health(c: &Cached<ToolStatus>) -> GenerateChipHealth {
    match c {
        Cached::Unknown | Cached::Loading => GenerateChipHealth::Pending,
        Cached::Ready(ToolStatus::Available { .. })
        | Cached::Stale {
            value: ToolStatus::Available { .. },
            ..
        } => GenerateChipHealth::Good,
        Cached::Ready(ToolStatus::Missing)
        | Cached::Ready(ToolStatus::Errored { .. })
        | Cached::Stale {
            value: ToolStatus::Missing | ToolStatus::Errored { .. },
            ..
        } => GenerateChipHealth::Bad,
    }
}

fn render_menu(state: &GenerateState, status: &StatusStore, frame: &mut Frame, area: Rect) {
    let focused = state.pane == Pane::Menu;
    let selected_count = match status.revsets.value() {
        Some(Revsets::Loaded(items)) => state.selected_heads_present(items).len(),
        _ => 0,
    };
    let title = if selected_count > 0 {
        format!("Changes  ({selected_count} selected)")
    } else {
        "Changes".to_string()
    };
    let block = theme::pane_block(&title, focused);
    let inner = block.inner(area);
    let inner_width = inner.width.saturating_sub(1) as usize;
    frame.render_widget(block, area);
    let (lines, scroll): (Vec<Line>, u16) = match status.revsets.value() {
        Some(Revsets::Loaded(items)) if !items.is_empty() => {
            let mut lines: Vec<Line> = Vec::new();
            let mut sel_start = 0u16;
            let mut sel_end = 0u16;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    lines.push(separator_line(inner_width));
                }
                let cursor = i == state.revset_selected;
                let head_selected = state.is_head_selected(&item.change_id);
                let row_start = lines.len() as u16;
                let row = revset_row_lines(item, cursor, head_selected, focused, inner_width);
                let row_end = row_start + row.len() as u16;
                if cursor {
                    sel_start = row_start;
                    sel_end = row_end.saturating_sub(1);
                }
                lines.extend(row);
            }
            // Only scroll when the selected item hits the viewport edge.
            let scroll = util::scroll_window(
                state.scroll_menu.get() as usize,
                sel_start as usize,
                sel_end as usize,
                inner.height as usize,
                lines.len(),
            )
            .offset as u16;
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
    cursor: bool,
    head_selected: bool,
    focused: bool,
    width: usize,
) -> Vec<Line<'static>> {
    // Gutter: 2 chars wide.
    // cursor and head_selected are orthogonal — show both when both are true.
    let gutter: &'static str = match (cursor, head_selected) {
        (true, true) => "▶●",
        (true, false) => "▶ ",
        (false, true) => " ●",
        (false, false) => "  ",
    };
    let gutter_style = if head_selected {
        theme::accent()
    } else {
        theme::muted()
    };

    let style = if cursor {
        theme::selected(focused)
    } else {
        theme::text()
    };

    let primary_lines = util::wrap_chars(&revset_primary(item), width.saturating_sub(2));
    let mut lines = vec![Line::from(vec![
        Span::styled(gutter, gutter_style),
        Span::styled(primary_lines.first().cloned().unwrap_or_default(), style),
    ])];
    for line in primary_lines.into_iter().skip(1) {
        lines.push(Line::from(Span::styled(format!("  {line}"), style)));
    }

    // Secondary description line when the row's headline is the bookmark.
    if !item.bookmarks.is_empty() && is_meaningful_description(&item.description) {
        lines.extend(
            util::wrap_chars(&item.description, width.saturating_sub(4))
                .into_iter()
                .map(|line| Line::from(Span::styled(format!("    {line}"), theme::muted()))),
        );
    }

    if item.commit_count > 1 {
        let summary = format!("{}c", item.commit_count);
        let (truncated, _) = util::truncate_ellipsis(&summary, width.saturating_sub(4));
        lines.push(Line::from(Span::styled(
            format!("    {truncated}"),
            theme::muted(),
        )));
    }

    lines
}

/// True when the form is in bulk mode (>=1 head selected) and `id` is `Head`.
fn is_bulk_head_field(state: &GenerateState, id: FieldId) -> bool {
    id == FieldId::Head && !state.selected_heads.is_empty()
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

        // In bulk mode the `head` field is derived from the selection and
        // rendered read-only with a `(from selection)` hint regardless of the
        // underlying picker state.
        if is_bulk_head_field(state, id) {
            let head_val = state.form.head();
            let display = if head_val.is_empty() {
                "(from selection)".to_string()
            } else {
                let (s, _) = util::truncate_ellipsis(head_val, w.saturating_sub(4));
                format!("{s}  (from selection)")
            };
            form_line(
                frame,
                inner,
                cy,
                scroll,
                Line::from(Span::styled(format!("{marker}{label}:"), style)),
            );
            cy += 1;
            form_line(
                frame,
                inner,
                cy,
                scroll,
                Line::from(Span::styled(format!("  {display}"), theme::muted())),
            );
            cy += 1;
            continue;
        }

        match state.form.field(id) {
            FieldState::Text(t) => {
                let value_w = w.saturating_sub(2);
                render_text_field(
                    frame, inner, &mut cy, scroll, t, label, marker, style, editing, value_w,
                );

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

fn form_scroll(state: &GenerateState, width: usize, visible: u16) -> u16 {
    let mut cy = 0u16;
    let mut focused_start = 0u16;
    let mut focused_end = 0u16;
    for (index, id) in FieldId::ALL.into_iter().enumerate() {
        if index > 0 {
            cy += 1;
        }
        let fh = form_field_height(state, id, width);
        if id == state.field_focus {
            focused_start = cy;
            focused_end = cy.saturating_add(fh).saturating_sub(1);
        }
        cy += fh;
    }
    let scroll = util::scroll_window(
        state.scroll_form.get() as usize,
        focused_start as usize,
        focused_end as usize,
        visible as usize,
        cy as usize,
    )
    .offset as u16;
    state.scroll_form.set(scroll);
    scroll
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

fn render_picker_modal(
    state: &GenerateState,
    picker: &form::PickerFieldState,
    frame: &mut Frame,
    area: Rect,
) {
    let width = area.width.saturating_sub(16).clamp(24, 52);
    let height = area.height.saturating_sub(6).clamp(6, 14);
    let rect = util::open_modal(frame, area, width, height);
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
        let window = util::scroll_window(
            picker.scroll.get(),
            picker.highlighted,
            picker.highlighted,
            rows,
            visible.len(),
        );
        picker.scroll.set(window.offset);
        for idx in window.range.clone() {
            let option = visible[idx];
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
            let label_width = (inner.width as usize)
                .saturating_sub(marker.chars().count())
                .saturating_sub(suffix.chars().count());
            let (label, _) = util::truncate_ellipsis(&option.label, label_width);
            lines.push(Line::from(Span::styled(
                format!("{marker}{label}{suffix}"),
                style,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_jj_op_dialog(dialog: &JjOpDialog, frame: &mut Frame, area: Rect) {
    let width = area.width.saturating_sub(16).clamp(34, 72);
    let height = area.height.saturating_sub(8).clamp(8, 12);
    let rect = util::open_modal(frame, area, width, height);
    let title = match dialog {
        JjOpDialog::Confirm(pending) => pending.title().to_string(),
        JjOpDialog::Error { title, .. } => title.clone(),
    };
    let block = theme::modal_block(title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = match dialog {
        JjOpDialog::Confirm(pending) => {
            let mut lines =
                wrapped_styled_lines("", &pending.question(), inner.width as usize, theme::text());
            lines.push(Line::from(""));
            lines.extend(wrapped_styled_lines(
                "",
                "This rewrites the stack. Conflicts will be probed and reverted.",
                inner.width as usize,
                theme::warning(),
            ));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Enter", theme::selected(true)),
                Span::styled(" confirm   ", theme::muted()),
                Span::styled("Esc", theme::selected(false)),
                Span::styled(" cancel", theme::muted()),
            ]));
            bounded_lines(lines, inner.height as usize)
        }
        JjOpDialog::Error { message, .. } => {
            let mut lines = vec![
                Line::from(Span::styled(
                    "Cannot run jj operation.",
                    theme::error().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            let footer = Line::from(vec![
                Span::styled("Enter/Esc", theme::selected(true)),
                Span::styled(" close", theme::muted()),
            ]);
            let msg_rows = (inner.height as usize).saturating_sub(lines.len() + 2);
            lines.extend(bounded_lines(
                wrapped_styled_lines("", message, inner.width as usize, theme::text()),
                msg_rows,
            ));
            lines.push(Line::from(""));
            lines.push(footer);
            bounded_lines(lines, inner.height as usize)
        }
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn bounded_lines(mut lines: Vec<Line<'static>>, max_rows: usize) -> Vec<Line<'static>> {
    if max_rows == 0 {
        return Vec::new();
    }
    if lines.len() <= max_rows {
        return lines;
    }
    lines.truncate(max_rows);
    if let Some(last) = lines.last_mut() {
        *last = Line::from(Span::styled("…", theme::muted()));
    }
    lines
}

// ========================= Bulk modal render ================================

fn render_bulk_modal(state: &GenerateState, frame: &mut Frame, area: Rect) {
    // The bulk modal takes most of the screen to accommodate the two-pane layout.
    let width = area.width.saturating_sub(8).max(40);
    let height = area.height.saturating_sub(4).max(12);
    let rect = util::open_modal(frame, area, width, height);

    match &state.bulk {
        BulkPhase::Idle => {}
        BulkPhase::Collecting => render_bulk_loading(
            frame,
            rect,
            "Collecting context…",
            state.selected_heads.len(),
        ),
        BulkPhase::Generating {
            inputs,
            drafts,
            warnings,
            next,
            total,
            ..
        } => render_bulk_generating(frame, rect, inputs, drafts, warnings, *next, *total),
        BulkPhase::Failed { message } => render_bulk_failed(frame, rect, message),
        BulkPhase::Review {
            plan,
            cursor,
            pushing,
            ..
        } => {
            render_bulk_review(state, plan, *cursor, *pushing, frame, rect);
        }
    }
}

fn render_bulk_loading(frame: &mut Frame, rect: Rect, status_msg: &str, selected_count: usize) {
    let block = theme::modal_block("Stacked PR Generation");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {status_msg}"),
            theme::accent().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  {} PR{} queued",
                selected_count,
                if selected_count == 1 { "" } else { "s" }
            ),
            theme::muted(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Esc", theme::selected(true)),
            Span::styled(" cancel", theme::muted()),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_bulk_generating(
    frame: &mut Frame,
    rect: Rect,
    inputs: &[crate::domain::StackPrInput],
    drafts: &[Option<crate::domain::StackDraft>],
    warnings: &[Vec<String>],
    next: usize,
    total: usize,
) {
    let block = theme::modal_block("Stacked PR Generation");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let done = drafts.iter().filter(|draft| draft.is_some()).count();
    let active = if total == 0 {
        0
    } else {
        next.saturating_sub(1).min(total.saturating_sub(1))
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "  Generating draft {} of {total}",
                done.saturating_add(1).min(total)
            ),
            theme::accent().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  {done}/{total} drafts ready"),
            theme::muted(),
        )),
        Line::from(""),
    ];

    let reserved = lines.len() + 2;
    let rows = (inner.height as usize).saturating_sub(reserved).max(1);
    let offset = active.saturating_add(1).saturating_sub(rows);
    for index in offset..total.min(offset + rows) {
        let Some(input) = inputs.get(index) else {
            continue;
        };
        let row_done = drafts.get(index).and_then(Option::as_ref).is_some();
        let row_warning = warnings.get(index).is_some_and(|w| !w.is_empty());
        let (marker, style) = if row_warning {
            ("! ", theme::warning())
        } else if row_done {
            ("✓ ", theme::success())
        } else if index == active {
            ("… ", theme::accent())
        } else {
            ("○ ", theme::muted())
        };
        let subject_w = inner.width.saturating_sub(8) as usize;
        let (subject, _) = util::truncate_ellipsis(&input.subject, subject_w);
        lines.push(Line::from(vec![
            Span::styled(format!("  {marker}"), style),
            Span::styled(format!("PR {} ", index + 1), theme::muted()),
            Span::styled(subject, theme::text()),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Esc", theme::selected(true)),
        Span::styled(" cancel", theme::muted()),
    ]));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_bulk_failed(frame: &mut Frame, rect: Rect, message: &str) {
    let block = theme::modal_block("Stacked PR Generation Failed");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Generation failed.",
            theme::error().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    let footer = Line::from(vec![
        Span::styled("Esc", theme::selected(true)),
        Span::styled(" close", theme::muted()),
    ]);
    let msg_rows = (inner.height as usize).saturating_sub(lines.len() + 2);
    lines.extend(bounded_lines(
        wrapped_styled_lines("  ", message, inner.width as usize, theme::text()),
        msg_rows,
    ));
    lines.push(Line::from(""));
    lines.push(footer);
    let lines = bounded_lines(lines, inner.height as usize);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_bulk_review(
    state: &GenerateState,
    plan: &crate::domain::StackPlan,
    cursor: usize,
    pushing: Option<usize>,
    frame: &mut Frame,
    rect: Rect,
) {
    let block = theme::modal_block("Review Stacked PRs");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [body_area, footer_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);

    frame.render_widget(
        Paragraph::new(bulk_review_footer_line(plan, footer_area.width)),
        footer_area,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    let gap = if body_area.width >= 80 { 2 } else { 1 };
    let list_width = (body_area.width.saturating_sub(gap) / 2).clamp(24, 44);
    let [list_shell, _gutter_area, right_area] = Layout::horizontal([
        Constraint::Length(list_width),
        Constraint::Length(gap),
        Constraint::Fill(1),
    ])
    .areas(body_area);

    let list_block = theme::pane_block("Stack", state.bulk_review_focus == BulkReviewFocus::List);
    let list_area = list_block.inner(list_shell);
    frame.render_widget(list_block, list_shell);

    // Split the right side vertically: Form (top, editable fields) and
    // Messages (bottom, blockers/warnings/results/last-action). Minimum
    // heights ensure both panes are usable at the 80x24 smoke-test floor.
    let messages_height = if right_area.height >= 16 {
        // Allocate at least 5 rows to Messages when there's room.
        right_area.height / 3
    } else if right_area.height >= 10 {
        // Compact: give Messages 4 rows (enough for a pane border + 2 content rows).
        4
    } else {
        // Very small: give Messages 3 rows minimum; Form keeps the rest.
        3
    };
    let form_height = right_area.height.saturating_sub(messages_height);
    let [form_shell, messages_shell] = Layout::vertical([
        Constraint::Length(form_height),
        Constraint::Length(messages_height),
    ])
    .areas(right_area);

    let form_block = theme::pane_block(
        "Selected PR",
        state.bulk_review_focus == BulkReviewFocus::Preview,
    );
    let form_area = form_block.inner(form_shell);
    frame.render_widget(form_block, form_shell);

    let messages_block = theme::pane_block(
        "Messages",
        state.bulk_review_focus == BulkReviewFocus::Messages,
    );
    let messages_area = messages_block.inner(messages_shell);
    frame.render_widget(messages_block, messages_shell);

    render_bulk_review_panes(
        state,
        plan,
        cursor,
        pushing,
        frame,
        list_area,
        form_area,
        messages_area,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_bulk_review_panes(
    state: &GenerateState,
    plan: &crate::domain::StackPlan,
    cursor: usize,
    pushing: Option<usize>,
    frame: &mut Frame,
    list_area: Rect,
    form_area: Rect,
    messages_area: Rect,
) {
    render_bulk_pr_list(state, plan, cursor, pushing, frame, list_area);
    render_bulk_pr_form(state, plan, cursor, frame, form_area);
    render_bulk_messages(state, plan, cursor, pushing, frame, messages_area);
}

fn bulk_review_footer_line(plan: &crate::domain::StackPlan, width: u16) -> Line<'static> {
    let blocker_count: usize = plan.items.iter().map(|item| item.blockers.len()).sum();
    let mut chips = vec![
        theme::StatusChip::plain(
            format!(
                "base: {}",
                plan.items
                    .first()
                    .map(|item| item.input.base.as_str())
                    .unwrap_or("-")
            ),
            0,
        ),
        theme::StatusChip::plain(format!("{} PRs", plan.items.len()), 1),
    ];

    if !plan.labels.is_empty() {
        chips.push(theme::StatusChip::plain(
            format!("labels: {}", plan.labels.join(", ")),
            2,
        ));
    }
    if !plan.milestone.is_empty() {
        chips.push(theme::StatusChip::plain(
            format!("milestone: {}", plan.milestone),
            3,
        ));
    }
    if blocker_count > 0 {
        chips.push(theme::StatusChip::plain(
            format!(
                "{blocker_count} blocker{}",
                if blocker_count == 1 { "" } else { "s" }
            ),
            4,
        ));
    }

    if width == 0 {
        Line::from(" ")
    } else {
        theme::status_line(chips, width)
    }
}

fn render_bulk_pr_list(
    state: &GenerateState,
    plan: &crate::domain::StackPlan,
    cursor: usize,
    pushing: Option<usize>,
    frame: &mut Frame,
    area: Rect,
) {
    let w = area.width as usize;
    let n = plan.items.len();

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut sel_y = 0u16;
    let mut sel_end = 0u16;

    for (i, item) in plan.items.iter().enumerate() {
        let focused = i == cursor;
        let pushing_current = pushing == Some(i);
        let list_focused = state.bulk_review_focus == BulkReviewFocus::List;
        let style = if focused {
            theme::selected(list_focused)
        } else if pushing_current {
            theme::accent()
        } else {
            theme::text()
        };
        let marker = if pushing_current {
            "⏳ "
        } else if focused {
            "▶ "
        } else {
            "  "
        };

        let row_start = lines.len() as u16;

        // Status badge.
        let status_badge = match &item.status {
            crate::domain::PrStatus::Pending => "○",
            crate::domain::PrStatus::Bookmarked => "◉",
            crate::domain::PrStatus::Pushed => "↑",
            crate::domain::PrStatus::Created { .. } => "✓",
            crate::domain::PrStatus::Failed { .. } => "✗",
        };
        let has_blocker = !item.blockers.is_empty();
        let has_warning = !item.warnings.is_empty() || !item.reuse_notes.is_empty();
        let flag = if has_blocker {
            "!"
        } else if has_warning {
            "~"
        } else {
            " "
        };

        // Primary row: marker + index + status + title. The title wraps inside
        // the row group, so the scroll span below can keep the full selected
        // row visible.
        let title_w = w.saturating_sub(5);
        let wrapped_title = util::wrap_chars(&item.title, title_w);
        for (line_index, title_line) in wrapped_title.iter().enumerate() {
            let prefix = if line_index == 0 {
                format!("{marker}{status_badge}{flag} ")
            } else {
                "     ".to_string()
            };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{title_line}"),
                style,
            )));
        }

        // Sub-row: bookmark.
        let bookmark_w = w.saturating_sub(4);
        let (bm_display, _) = util::truncate_ellipsis(&item.bookmark, bookmark_w);
        lines.push(Line::from(Span::styled(
            format!("  ↳ {bm_display}"),
            if focused {
                theme::selected(false)
            } else {
                theme::muted()
            },
        )));

        if i + 1 < n {
            lines.push(separator_line(w.saturating_sub(2)));
        }

        if focused {
            sel_y = row_start;
            sel_end = (lines.len() as u16).saturating_sub(1);
        }
    }

    if lines.is_empty() {
        lines.push(placeholder_line("No PRs in plan."));
    }

    // Natural scroll: only move the window when the cursor crosses the edge.
    let scroll = util::scroll_window(
        state.bulk_list_scroll.get() as usize,
        sel_y as usize,
        sel_end as usize,
        area.height as usize,
        lines.len(),
    )
    .offset as u16;
    state.bulk_list_scroll.set(scroll);

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

fn render_bulk_pr_form(
    state: &GenerateState,
    plan: &crate::domain::StackPlan,
    cursor: usize,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(item) = plan.items.get(cursor) else {
        frame.render_widget(Paragraph::new(placeholder_line("No PR selected.")), area);
        return;
    };

    let w = area.width as usize;
    let editor = &state.bulk_editor;
    let value_w = w.saturating_sub(2);

    let scroll = bulk_form_scroll(state, w, area.height);

    // --- Render pass ---
    // Use the same inner rect as `area` (the caller already passed the inner
    // rect from the pane block).
    let inner = area;
    let mut cy = 0u16;

    // Read-only head / base.
    let (head_s, _) = util::truncate_ellipsis(&item.input.head, w.saturating_sub(8));
    let (base_s, _) = util::truncate_ellipsis(&item.input.base, w.saturating_sub(8));
    form_line(
        frame,
        inner,
        cy,
        scroll,
        Line::from(vec![
            Span::styled("  head: ", theme::muted()),
            Span::styled(head_s, theme::text()),
            Span::styled("  (read-only)", theme::muted()),
        ]),
    );
    cy += 1;
    form_line(
        frame,
        inner,
        cy,
        scroll,
        Line::from(vec![
            Span::styled("  base: ", theme::muted()),
            Span::styled(base_s, theme::text()),
            Span::styled("  (read-only)", theme::muted()),
        ]),
    );
    cy += 1;

    form_line(
        frame,
        inner,
        cy,
        scroll,
        separator_line(w.saturating_sub(2)),
    );
    cy += 1;

    // Editable fields via the shared renderer.
    for field in [
        BulkItemField::Title,
        BulkItemField::Branch,
        BulkItemField::Description,
    ] {
        let preview_focused = state.bulk_review_focus == BulkReviewFocus::Preview || editor.editing;
        let focused = editor.field_focus == field;
        let editing = focused && editor.editing;
        let marker = if preview_focused && focused {
            "▶ "
        } else {
            "  "
        };
        let style = if preview_focused && focused {
            theme::selected(true)
        } else {
            theme::muted()
        };
        let t = editor.field(field);
        render_text_field(
            frame,
            inner,
            &mut cy,
            scroll,
            t,
            field.label(),
            marker,
            style,
            editing,
            value_w,
        );
    }

    // Annotation, result, and last-action lines are now rendered in the
    // separate Messages pane (render_bulk_messages).
}

/// Render the Messages pane: blockers, warnings, reuse notes, push results,
/// and last-action text. The pane has its own scroll offset so the user can
/// navigate to it and scroll with Up/Down/j/k even when the Description is long.
fn render_bulk_messages(
    state: &GenerateState,
    plan: &crate::domain::StackPlan,
    cursor: usize,
    pushing: Option<usize>,
    frame: &mut Frame,
    area: Rect,
) {
    if area.height == 0 {
        return;
    }
    let Some(item) = plan.items.get(cursor) else {
        frame.render_widget(Paragraph::new(placeholder_line("No PR selected.")), area);
        return;
    };

    let w = area.width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Push progress at the top when actively pushing this item.
    if pushing == Some(cursor) {
        let message = if state.last_action.as_deref() == Some("checking current PR state") {
            "checking: bookmarks and existing PRs"
        } else {
            "pushing: bookmark -> push -> create PR"
        };
        lines.extend(wrapped_styled_lines(
            "  ",
            message,
            w,
            theme::accent().add_modifier(Modifier::BOLD),
        ));
    }

    lines.extend(bulk_annotation_lines(item, w));
    let result_lines = bulk_result_lines(plan, w);
    let has_result = !result_lines.is_empty();
    if has_result && !lines.is_empty() {
        lines.push(separator_line(w.saturating_sub(2)));
    }
    lines.extend(result_lines);

    // Show the last-action note only when there is no definitive result section.
    // Avoids echoing "stack push complete" or the push error a second time.
    if !has_result && pushing != Some(cursor) {
        if let Some(note) = &state.last_action {
            lines.extend(bulk_note_lines(note, w));
        }
    }

    if lines.is_empty() {
        lines.push(placeholder_line("no messages"));
    }

    // Natural scroll for the messages pane. When the pane has focus, the user
    // controls scroll_offset via Up/Down/j/k. We clamp and persist via
    // bulk_messages_scroll.
    let total = lines.len();
    let visible = area.height as usize;
    let cur_offset = state.bulk_messages_scroll.get();
    // Clamp the persisted offset to the valid range (content may have shrunk).
    let max_off = total.saturating_sub(visible);
    let clamped = cur_offset.min(max_off);
    if clamped != cur_offset {
        state.bulk_messages_scroll.set(clamped);
    }
    let scroll = clamped as u16;

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

fn bulk_form_scroll(state: &GenerateState, width: usize, visible: u16) -> u16 {
    let editor = &state.bulk_editor;
    let value_w = width.saturating_sub(2);
    let mut cy = 0u16;
    cy += 2; // read-only head + base
    cy += 1; // separator

    let mut focused_start = 0u16;
    let mut focused_end = 0u16;
    for field in [
        BulkItemField::Title,
        BulkItemField::Branch,
        BulkItemField::Description,
    ] {
        let is_focused = editor.field_focus == field;
        let editing = is_focused && editor.editing;
        let t = editor.field(field);
        let label_row = cy;
        cy += 1; // label
        let value_h = if t.multiline {
            multiline_value_height(t, value_w, editing)
        } else {
            1
        };
        cy = cy.saturating_add(value_h);
        if is_focused {
            focused_start = label_row;
            focused_end = cy.saturating_sub(1);
        }
    }

    let (target_start, target_end) = (focused_start, focused_end);

    let scroll = util::scroll_window(
        state.bulk_form_scroll.get() as usize,
        target_start as usize,
        target_end as usize,
        visible as usize,
        cy as usize,
    )
    .offset as u16;
    state.bulk_form_scroll.set(scroll);
    scroll
}

fn bulk_annotation_lines(item: &crate::domain::StackPlanItem, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for blocker in &item.blockers {
        lines.extend(wrapped_styled_lines("  ! ", blocker, width, theme::error()));
    }
    for warning in &item.warnings {
        lines.extend(wrapped_styled_lines(
            "  ~ ",
            warning,
            width,
            theme::warning(),
        ));
    }
    for note in &item.reuse_notes {
        lines.extend(wrapped_styled_lines("  ~ ", note, width, theme::muted()));
    }
    lines
}

fn bulk_result_lines(plan: &crate::domain::StackPlan, width: usize) -> Vec<Line<'static>> {
    if plan.items.is_empty() {
        return Vec::new();
    }

    let all_created = plan
        .items
        .iter()
        .all(|item| matches!(item.status, crate::domain::PrStatus::Created { .. }));
    let any_failed = plan
        .items
        .iter()
        .enumerate()
        .find_map(|(index, item)| match &item.status {
            crate::domain::PrStatus::Failed { step, message } => {
                Some((index, step.label(), message))
            }
            _ => None,
        });

    if !all_created && any_failed.is_none() {
        return Vec::new();
    }

    let mut lines = vec![section_heading_line("result")];
    if all_created {
        lines.push(Line::from(Span::styled(
            "  stack push complete",
            theme::success().add_modifier(Modifier::BOLD),
        )));
    } else if let Some((index, step, message)) = any_failed {
        lines.push(Line::from(Span::styled(
            format!("  PR {} failed at {}", index + 1, step),
            theme::error().add_modifier(Modifier::BOLD),
        )));
        lines.extend(wrapped_styled_lines("  ", message, width, theme::error()));
    }
    for (index, item) in plan.items.iter().enumerate() {
        if let crate::domain::PrStatus::Created { url } = &item.status {
            lines.push(kv_line_fit(&format!("pr {}", index + 1), url, width));
        }
    }
    lines
}

fn bulk_note_lines(note: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![separator_line(width.saturating_sub(2))];
    lines.extend(wrapped_styled_lines("  ", note, width, theme::muted()));
    lines
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
        GeneratePhase::DraftReady { draft, prompt } => {
            preview_draft_lines(state, draft, prompt, inner.width)
        }
        GeneratePhase::Confirming {
            draft,
            prompt,
            commands,
        } => preview_confirming_lines(state, draft, prompt, commands, inner.width),
        GeneratePhase::Executing { draft } => preview_executing_lines(draft),
        GeneratePhase::JjMutating { op, summary } => preview_jj_mutating_lines(*op, summary),
        GeneratePhase::Done { url } => preview_done_lines(url),
        GeneratePhase::Failed { message } => preview_failed_lines(message, inner.width),
    });
    lines.extend(next_step_lines(state, status));
    if let Some(hint) = &state.last_action {
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
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        status_line(&format!(
            "Generated draft for {}",
            fmt_or_dash(state.form.branch())
        )),
        hint_line("The generated draft is editable in the center pane."),
    ];
    lines.extend(draft_lines(state, draft, width));
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

fn preview_failed_lines(message: &str, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        "  failed",
        theme::error().add_modifier(Modifier::BOLD),
    ))];
    lines.extend(wrapped_styled_lines(
        "  ",
        message,
        width as usize,
        theme::error(),
    ));
    lines
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
                    for wrapped in util::wrap_chars(line, width.saturating_sub(2)) {
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
                let (truncated, _) = util::truncate_ellipsis(&p.value, width.saturating_sub(2));
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

fn selected_revset<'a>(
    state: &GenerateState,
    status: &'a StatusStore,
) -> Option<&'a RevsetSummary> {
    let Some(Revsets::Loaded(items)) = status.revsets.value() else {
        return None;
    };
    items.get(state.revset_selected)
}

fn draft_lines(state: &GenerateState, draft: &GeneratedDraft, width: u16) -> Vec<Line<'static>> {
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
            lines.extend(wrapped_styled_lines(
                "  ",
                line,
                width as usize,
                theme::text(),
            ));
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
        let (truncated, cut) = util::truncate_ellipsis(command, cmd_width);
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

fn render_help(state: &GenerateState, frame: &mut Frame, area: Rect) {
    use theme::HelpHint;
    let hints: Vec<HelpHint> = if state.input_mode == InputMode::Editing {
        match state.form.field(state.field_focus).kind() {
            FieldKind::Text { multiline: true } => vec![
                HelpHint::primary("Ctrl+S", "commit"),
                HelpHint::new("Alt+Enter", "commit"),
                HelpHint::new("Enter", "newline"),
                HelpHint::new("Esc", "cancel"),
            ],
            FieldKind::Text { multiline: false } => vec![
                HelpHint::primary("Enter", "commit"),
                HelpHint::new("Esc", "cancel"),
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
    } else if !matches!(state.bulk, BulkPhase::Idle) {
        // Bulk modal is open — show bulk-specific hints.
        match &state.bulk {
            BulkPhase::Collecting | BulkPhase::Generating { .. } => {
                vec![theme::HelpHint::primary("Esc", "cancel")]
            }
            BulkPhase::Review {
                pushing: Some(_), ..
            } => {
                vec![
                    theme::HelpHint::new("j/k", "navigate"),
                    theme::HelpHint::primary("Wait", "push running…"),
                ]
            }
            BulkPhase::Review { .. } => {
                if state.bulk_editor.editing {
                    let multiline = state.bulk_editor.field_focus == BulkItemField::Description;
                    if multiline {
                        vec![
                            theme::HelpHint::primary("Ctrl+S", "commit"),
                            theme::HelpHint::new("Alt+Enter", "commit"),
                            theme::HelpHint::new("Enter", "newline"),
                            theme::HelpHint::new("Esc", "cancel"),
                        ]
                    } else {
                        vec![
                            theme::HelpHint::primary("Enter", "commit"),
                            theme::HelpHint::new("Esc", "cancel"),
                        ]
                    }
                } else if state.bulk_review_focus == BulkReviewFocus::List {
                    vec![
                        theme::HelpHint::primary("Enter/→", "form"),
                        theme::HelpHint::new("j/k", "select PR"),
                        theme::HelpHint::new("p", "push current"),
                        theme::HelpHint::new("P", "push all"),
                        theme::HelpHint::new("r", "refresh"),
                        theme::HelpHint::new("Esc", "close"),
                    ]
                } else if state.bulk_review_focus == BulkReviewFocus::Preview {
                    vec![
                        theme::HelpHint::primary("Enter/i", "edit"),
                        theme::HelpHint::new("j/k", "fields"),
                        theme::HelpHint::new("Tab", "fields"),
                        theme::HelpHint::new("→", "messages"),
                        theme::HelpHint::new("p", "push current"),
                        theme::HelpHint::new("P", "push all"),
                        theme::HelpHint::new("r", "refresh"),
                        theme::HelpHint::new("Esc/←", "list"),
                    ]
                } else {
                    // Messages pane focus
                    vec![
                        theme::HelpHint::primary("j/k", "scroll"),
                        theme::HelpHint::new("PgUp/PgDn", "page"),
                        theme::HelpHint::new("←/Esc", "form"),
                    ]
                }
            }
            BulkPhase::Failed { .. } => vec![theme::HelpHint::primary("Esc", "close")],
            BulkPhase::Idle => unreachable!(),
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
            if !state.has_busy_job() && state.jj_op_dialog.is_none() {
                hints.push(HelpHint::new("space", "toggle head"));
                hints.push(HelpHint::new("s", "squash below"));
                hints.push(HelpHint::new("J/K", "move"));
                if !state.selected_heads.is_empty() {
                    hints.push(HelpHint::new("G", "review stack"));
                }
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
    if state.pane != Pane::Menu && !state.has_busy_job() {
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
        // selected_heads, bulk, bulk_editor, bulk_list_scroll, bulk_form_scroll,
        // and bulk_messages_scroll default to their zero values via GenerateState::new
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

    fn named_revset(change_id: &str) -> RevsetSummary {
        RevsetSummary {
            label: format!("trunk()..{change_id}"),
            change_id: change_id.into(),
            commit_id: format!("{change_id}-commit"),
            bookmarks: Vec::new(),
            description: String::new(),
            description_body: String::new(),
            author: String::new(),
            stats: String::new(),
            commit_count: 1,
            commit_ids: vec![format!("{change_id}-commit")],
            change_ids: vec![change_id.into()],
            recent_log: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn selected_heads_present_follows_display_order_and_drops_stale_ids() {
        let mut state = GenerateState::new("main".into());
        // Selected in an arbitrary order, including an id that no longer exists.
        state.selected_heads = vec!["old".into(), "gone".into(), "new".into()];

        // A reorder of the revset list must not change which heads resolve, only
        // the order they come back in (always newest-first display order).
        let revsets = vec![
            named_revset("new"),
            named_revset("mid"),
            named_revset("old"),
        ];
        let present: Vec<&str> = state
            .selected_heads_present(&revsets)
            .into_iter()
            .map(|r| r.change_id.as_str())
            .collect();

        // "gone" is dropped; "new"/"old" come back in display (newest-first) order.
        assert_eq!(present, vec!["new", "old"]);
    }
}
