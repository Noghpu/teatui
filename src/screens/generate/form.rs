use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::TextArea;

use crate::domain::{RepoOptions, Revsets, StatusStore};

pub const HEAD_BEHIND_BASE_WARNING: &str = "head is older than base";
pub const BASE_AHEAD_OF_HEAD_WARNING: &str = "base is newer than head";

impl std::fmt::Debug for TextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldState")
            .field("initial", &self.initial)
            .field("value", &self.value)
            .field("buffer", &self.buffer)
            .field("dirty", &self.dirty)
            .field("errors", &self.errors)
            .field("multiline", &self.multiline)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldId {
    #[default]
    Head,
    BranchName,
    Base,
    Title,
    Description,
    Labels,
    Assignees,
    Milestone,
}

impl FieldId {
    pub const ALL: [FieldId; 8] = [
        FieldId::Head,
        FieldId::Base,
        FieldId::BranchName,
        FieldId::Title,
        FieldId::Description,
        FieldId::Labels,
        FieldId::Assignees,
        FieldId::Milestone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FieldId::Head => "head",
            FieldId::BranchName => "branch",
            FieldId::Base => "base",
            FieldId::Title => "title",
            FieldId::Description => "description",
            FieldId::Labels => "labels",
            FieldId::Assignees => "assignees",
            FieldId::Milestone => "milestone",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + 1).min(Self::ALL.len() - 1)]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[idx.saturating_sub(1)]
    }

    /// Position in `ALL` — used to index the per-field history and snapshot.
    fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text { multiline: bool },
    Picker { multi_select: bool, optional: bool },
}

#[derive(Debug, Clone)]
pub enum FieldState {
    Text(Box<TextFieldState>),
    Picker(PickerFieldState),
}

impl FieldState {
    pub fn kind(&self) -> FieldKind {
        match self {
            FieldState::Text(t) => FieldKind::Text {
                multiline: t.multiline,
            },
            FieldState::Picker(p) => FieldKind::Picker {
                multi_select: p.multi_select,
                optional: p.optional,
            },
        }
    }

    pub fn value(&self) -> &str {
        match self {
            FieldState::Text(t) => &t.value,
            FieldState::Picker(p) => &p.value,
        }
    }

    pub fn values(&self) -> Vec<String> {
        match self {
            FieldState::Text(t) => vec![t.value.clone()],
            FieldState::Picker(p) => p.committed.clone(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        match self {
            FieldState::Text(t) => t.dirty,
            FieldState::Picker(p) => p.dirty,
        }
    }

    pub fn errors(&self) -> &[String] {
        match self {
            FieldState::Text(t) => &t.errors,
            FieldState::Picker(p) => &p.errors,
        }
    }

    pub fn set_value(&mut self, value: String) {
        match self {
            FieldState::Text(t) => t.set_value(value),
            FieldState::Picker(p) => p.set_values(if value.is_empty() {
                Vec::new()
            } else {
                vec![value]
            }),
        }
    }

    pub fn set_values(&mut self, values: Vec<String>) {
        match self {
            FieldState::Text(t) => t.set_value(values.join(", ")),
            FieldState::Picker(p) => p.set_values(values),
        }
    }

    pub fn begin_edit(&mut self) {
        match self {
            FieldState::Text(t) => t.begin_edit(),
            FieldState::Picker(p) => p.begin_edit(),
        }
    }

    pub fn commit(&mut self) {
        match self {
            FieldState::Text(t) => t.commit(),
            FieldState::Picker(p) => p.commit(),
        }
    }

    pub fn cancel(&mut self) {
        match self {
            FieldState::Text(t) => t.cancel(),
            FieldState::Picker(p) => p.cancel(),
        }
    }
}

#[derive(Clone)]
pub struct TextFieldState {
    initial: String,
    pub value: String,
    pub buffer: String,
    pub editor: TextArea<'static>,
    pub dirty: bool,
    pub errors: Vec<String>,
    pub multiline: bool,
}

impl TextFieldState {
    pub fn new(value: String, multiline: bool) -> Self {
        let editor = text_area_from(&value);
        Self {
            initial: value.clone(),
            value: value.clone(),
            buffer: value,
            editor,
            dirty: false,
            errors: Vec::new(),
            multiline,
        }
    }

    pub fn set_value(&mut self, value: String) {
        self.initial = value.clone();
        self.value = value.clone();
        self.buffer = value.clone();
        self.editor = text_area_from(&value);
        self.dirty = false;
        self.errors.clear();
    }

    pub fn begin_edit(&mut self) {
        self.buffer = self.value.clone();
        self.editor = text_area_from(&self.buffer);
    }

    pub fn commit(&mut self) {
        self.buffer = self.editor.lines().join("\n");
        self.value = self.buffer.clone();
        self.dirty = self.value != self.initial;
    }

    pub fn cancel(&mut self) {
        self.buffer = self.value.clone();
        self.editor = text_area_from(&self.buffer);
    }

    pub fn input(&mut self, key: KeyEvent) {
        if !self.multiline && key.code == KeyCode::Enter {
            return;
        }
        self.editor.input(key);
        self.buffer = self.editor.lines().join("\n");
    }
}

fn text_area_from(value: &str) -> TextArea<'static> {
    let lines: Vec<String> = if value.is_empty() {
        vec![String::new()]
    } else {
        value.lines().map(str::to_string).collect()
    };
    TextArea::new(lines)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOption {
    pub label: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PickerFieldState {
    initial: Vec<String>,
    committed: Vec<String>,
    draft: Vec<String>,
    pub value: String,
    pub options: Vec<PickerOption>,
    pub filter: String,
    pub highlighted: usize,
    /// Top index of the scrolled option window. A `Cell` so render can keep it
    /// in sync with `highlighted` without a mutable borrow; edge-clamped there.
    pub scroll: Cell<usize>,
    pub multi_select: bool,
    pub optional: bool,
    pub editing: bool,
    pub errors: Vec<String>,
    pub dirty: bool,
}

impl PickerFieldState {
    pub fn new(
        committed: Vec<String>,
        multi_select: bool,
        optional: bool,
        options: Vec<PickerOption>,
    ) -> Self {
        let value = committed.join(", ");
        Self {
            initial: committed.clone(),
            committed: committed.clone(),
            draft: committed,
            value,
            options,
            filter: String::new(),
            highlighted: 0,
            scroll: Cell::new(0),
            multi_select,
            optional,
            editing: false,
            errors: Vec::new(),
            dirty: false,
        }
    }

    pub fn set_values(&mut self, values: Vec<String>) {
        self.initial = values.clone();
        self.committed = values.clone();
        self.draft = values;
        self.value = self.committed.join(", ");
        self.filter.clear();
        self.highlighted = 0;
        self.editing = false;
        self.dirty = false;
        self.errors.clear();
    }

    pub fn set_options(&mut self, options: Vec<PickerOption>) {
        self.options = options;
        self.highlighted = self
            .highlighted
            .min(self.visible_options().len().saturating_sub(1));
    }

    pub fn begin_edit(&mut self) {
        self.draft = self.committed.clone();
        self.filter.clear();
        // Open on the current selection rather than the top of the list. The
        // filter was just cleared, so `visible_options` is `options` in order.
        self.highlighted = self
            .committed
            .first()
            .and_then(|value| self.options.iter().position(|o| &o.value == value))
            .unwrap_or(0);
        self.editing = true;
    }

    pub fn commit(&mut self) {
        if !self.multi_select
            && let Some(option) = self.visible_options().get(self.highlighted)
        {
            self.draft = vec![option.value.clone()];
        }
        self.committed = self.draft.clone();
        self.value = self.committed.join(", ");
        self.dirty = self.committed != self.initial;
        self.editing = false;
    }

    pub fn cancel(&mut self) {
        self.draft = self.committed.clone();
        self.filter.clear();
        self.highlighted = 0;
        self.editing = false;
    }

    pub fn visible_options(&self) -> Vec<&PickerOption> {
        let filter = self.filter.to_lowercase();
        self.options
            .iter()
            .filter(|option| {
                filter.is_empty()
                    || option.label.to_lowercase().contains(&filter)
                    || option.value.to_lowercase().contains(&filter)
            })
            .collect()
    }

    pub fn move_highlight(&mut self, delta: isize) {
        let count = self.visible_options().len();
        if count == 0 {
            self.highlighted = 0;
            return;
        }
        let next = self.highlighted.saturating_add_signed(delta);
        self.highlighted = next.min(count - 1);
    }

    pub fn toggle_highlighted(&mut self) {
        if !self.multi_select {
            return;
        }
        let Some(value) = self
            .visible_options()
            .get(self.highlighted)
            .map(|o| o.value.clone())
        else {
            return;
        };
        if let Some(pos) = self.draft.iter().position(|v| v == &value) {
            self.draft.remove(pos);
        } else {
            self.draft.push(value);
        }
    }

    pub fn draft_contains(&self, value: &str) -> bool {
        self.draft.iter().any(|v| v == value)
    }

    pub fn input_filter(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.filter.push(c);
                self.highlighted = 0;
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.highlighted = 0;
            }
            _ => {}
        }
    }
}

/// Undo/redo stacks for one value. `record` saves the prior value and drops the
/// redo tail (a fresh change invalidates pending redos); `undo`/`redo` shuttle a
/// value between the two stacks, returning the one to apply.
#[derive(Debug, Clone, Default)]
struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T> History<T> {
    fn record(&mut self, prev: T) {
        self.undo.push(prev);
        self.redo.clear();
    }

    fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
}

/// The committed values of every field in `FieldId::ALL` order — the unit of
/// whole-form undo/redo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FormSnapshot([Vec<String>; 8]);

/// Whole-form and per-field undo/redo history for a [`PrForm`]. Recorded
/// through [`PrForm::edit`] so a single edit (e.g. the LLM overwriting several
/// fields at once) is one whole-form undo while still being undoable field by
/// field.
#[derive(Debug, Clone, Default)]
struct FormHistory {
    form: History<FormSnapshot>,
    fields: [History<Vec<String>>; 8],
}

#[derive(Debug, Clone)]
pub struct PrForm {
    pub head: FieldState,
    pub branch_name: FieldState,
    pub base: FieldState,
    pub title: FieldState,
    pub description: FieldState,
    pub labels: FieldState,
    pub assignees: FieldState,
    pub milestone: FieldState,
    /// Configured default base (e.g. `main@origin`). Surfaced as the last
    /// option in the base picker so the immutable trunk boundary — which the
    /// `trunk()..@` change list excludes — is still selectable.
    default_base: String,
    history: FormHistory,
}

impl Default for PrForm {
    fn default() -> Self {
        Self {
            head: picker(false, false),
            branch_name: text("", false),
            base: picker(false, false),
            title: text("", false),
            description: text("", true),
            labels: picker(true, true),
            assignees: picker(true, true),
            milestone: picker(false, true),
            default_base: String::new(),
            history: FormHistory::default(),
        }
    }
}

impl PrForm {
    pub fn new(default_base: String) -> Self {
        let mut form = Self::default();
        form.base.set_value(default_base.clone());
        form.default_base = default_base;
        form
    }

    pub fn field(&self, id: FieldId) -> &FieldState {
        match id {
            FieldId::Head => &self.head,
            FieldId::BranchName => &self.branch_name,
            FieldId::Base => &self.base,
            FieldId::Title => &self.title,
            FieldId::Description => &self.description,
            FieldId::Labels => &self.labels,
            FieldId::Assignees => &self.assignees,
            FieldId::Milestone => &self.milestone,
        }
    }

    pub fn field_mut(&mut self, id: FieldId) -> &mut FieldState {
        match id {
            FieldId::Head => &mut self.head,
            FieldId::BranchName => &mut self.branch_name,
            FieldId::Base => &mut self.base,
            FieldId::Title => &mut self.title,
            FieldId::Description => &mut self.description,
            FieldId::Labels => &mut self.labels,
            FieldId::Assignees => &mut self.assignees,
            FieldId::Milestone => &mut self.milestone,
        }
    }

    /// Run a mutation of the form and record it so it can be undone. The
    /// pre-edit snapshot is pushed to the whole-form history, and every field
    /// that changed pushes its prior value to that field's history — so one
    /// edit is a single `u` undo yet still reversible field by field with `U`.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.snapshot();
        let result = f(self);
        let after = self.snapshot();
        if before != after {
            for id in FieldId::ALL {
                let i = id.index();
                if before.0[i] != after.0[i] {
                    self.history.fields[i].record(before.0[i].clone());
                }
            }
            self.history.form.record(before);
        }
        result
    }

    /// Undo the most recent [`edit`](Self::edit), restoring every field to its
    /// pre-edit value. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(prev) = self.history.form.undo(current) else {
            return false;
        };
        self.restore(&prev);
        true
    }

    /// Redo the most recently undone whole-form edit.
    pub fn redo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(next) = self.history.form.redo(current) else {
            return false;
        };
        self.restore(&next);
        true
    }

    /// Undo the most recent change to a single field, leaving the others alone.
    pub fn undo_field(&mut self, id: FieldId) -> bool {
        let i = id.index();
        let current = self.field(id).values();
        let Some(prev) = self.history.fields[i].undo(current) else {
            return false;
        };
        self.field_mut(id).set_values(prev);
        true
    }

    /// Redo the most recently undone change to a single field.
    pub fn redo_field(&mut self, id: FieldId) -> bool {
        let i = id.index();
        let current = self.field(id).values();
        let Some(next) = self.history.fields[i].redo(current) else {
            return false;
        };
        self.field_mut(id).set_values(next);
        true
    }

    fn snapshot(&self) -> FormSnapshot {
        let mut snap = FormSnapshot::default();
        for id in FieldId::ALL {
            snap.0[id.index()] = self.field(id).values();
        }
        snap
    }

    fn restore(&mut self, snap: &FormSnapshot) {
        for id in FieldId::ALL {
            let values = &snap.0[id.index()];
            if self.field(id).values() != *values {
                self.field_mut(id).set_values(values.clone());
            }
        }
    }

    pub fn head(&self) -> &str {
        self.head.value()
    }

    pub fn branch(&self) -> &str {
        self.branch_name.value()
    }

    pub fn base(&self) -> &str {
        self.base.value()
    }

    pub fn title(&self) -> &str {
        self.title.value()
    }

    pub fn description(&self) -> &str {
        self.description.value()
    }

    pub fn labels(&self) -> Vec<String> {
        self.labels.values()
    }

    pub fn assignees(&self) -> Vec<String> {
        self.assignees.values()
    }

    pub fn milestone(&self) -> &str {
        self.milestone.value()
    }

    pub fn validate(&mut self) -> bool {
        for id in FieldId::ALL {
            match self.field_mut(id) {
                FieldState::Text(t) => t.errors.clear(),
                FieldState::Picker(p) => p.errors.clear(),
            }
        }
        self.require(FieldId::Head);
        self.require(FieldId::BranchName);
        self.require(FieldId::Base);
        self.require(FieldId::Title);
        if self.description().trim().is_empty()
            && let FieldState::Text(t) = self.field_mut(FieldId::Description)
        {
            t.errors.push("warning: empty".into());
        }
        FieldId::ALL
            .into_iter()
            .filter(|id| *id != FieldId::Description)
            .all(|id| self.field(id).errors().is_empty())
    }

    fn require(&mut self, id: FieldId) {
        if self.field(id).value().trim().is_empty() {
            match self.field_mut(id) {
                FieldState::Text(t) => t.errors.push("required".into()),
                FieldState::Picker(p) => p.errors.push("required".into()),
            }
        }
    }

    pub fn sync_options(&mut self, status: &StatusStore) {
        if let Some(Revsets::Loaded(items)) = status.revsets.value() {
            set_picker_options(
                &mut self.head,
                items
                    .iter()
                    .map(|item| PickerOption {
                        label: super::revset_primary(item),
                        value: item.change_id.clone(),
                        enabled: true,
                    })
                    .collect(),
            );
        }
        if let Some(Revsets::Loaded(items)) = status.revsets.value() {
            // The change list is `trunk()..@`, which excludes the trunk
            // boundary itself. Append the configured default base (a remote
            // ref like `main@origin`) as the oldest selectable base so the
            // immutable trunk is reachable from the picker. It goes last
            // because it is the oldest ancestor — keeping the list ordered
            // newest→oldest so the head/base relative-order warning holds.
            let mut options: Vec<PickerOption> = items
                .iter()
                .map(|item| PickerOption {
                    label: super::revset_primary(item),
                    value: item.change_id.clone(),
                    enabled: true,
                })
                .collect();
            if !self.default_base.is_empty() {
                options.push(PickerOption {
                    label: self.default_base.clone(),
                    value: self.default_base.clone(),
                    enabled: true,
                });
            }
            set_picker_options(&mut self.base, options);
        }
        if let Some(options) = status.repo_options.value() {
            sync_repo_options(self, options);
        }
    }

    pub fn relative_order_warning(&self, id: FieldId) -> Option<&'static str> {
        let head = self.picker_option_index(FieldId::Head, self.head())?;
        let base = self.picker_option_index(FieldId::Base, self.base())?;
        if head <= base {
            return None;
        }
        match id {
            FieldId::Head => Some(HEAD_BEHIND_BASE_WARNING),
            FieldId::Base => Some(BASE_AHEAD_OF_HEAD_WARNING),
            _ => None,
        }
    }

    pub fn picker_option_warning(&self, id: FieldId, option_value: &str) -> Option<&'static str> {
        let option = self.picker_option_index(id, option_value)?;
        match id {
            FieldId::Head => {
                let base = self.picker_option_index(FieldId::Base, self.base())?;
                (option > base).then_some(HEAD_BEHIND_BASE_WARNING)
            }
            FieldId::Base => {
                let head = self.picker_option_index(FieldId::Head, self.head())?;
                (option < head).then_some(BASE_AHEAD_OF_HEAD_WARNING)
            }
            _ => None,
        }
    }

    fn picker_option_index(&self, id: FieldId, value: &str) -> Option<usize> {
        if value.is_empty() {
            return None;
        }
        let FieldState::Picker(picker) = self.field(id) else {
            return None;
        };
        picker
            .options
            .iter()
            .position(|option| option.value == value)
    }
}

fn sync_repo_options(form: &mut PrForm, options: &RepoOptions) {
    set_picker_options(
        &mut form.labels,
        options
            .labels
            .iter()
            .map(String::as_str)
            .map(option)
            .collect(),
    );
    set_picker_options(
        &mut form.assignees,
        options
            .assignees
            .iter()
            .map(String::as_str)
            .map(option)
            .collect(),
    );
    set_picker_options(
        &mut form.milestone,
        options
            .milestones
            .iter()
            .map(String::as_str)
            .map(option)
            .collect(),
    );
}

fn text(value: &str, multiline: bool) -> FieldState {
    FieldState::Text(Box::new(TextFieldState::new(value.to_string(), multiline)))
}

fn picker(multi_select: bool, optional: bool) -> FieldState {
    FieldState::Picker(PickerFieldState::new(
        Vec::new(),
        multi_select,
        optional,
        Vec::new(),
    ))
}

fn set_picker_options(field: &mut FieldState, options: Vec<PickerOption>) {
    if let FieldState::Picker(p) = field {
        p.set_options(options);
    }
}

fn option(value: &str) -> PickerOption {
    PickerOption {
        label: value.to_string(),
        value: value.to_string(),
        enabled: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn revset(change_id: &str) -> crate::domain::RevsetSummary {
        crate::domain::RevsetSummary {
            label: format!("trunk()..{change_id}"),
            change_id: change_id.to_string(),
            commit_id: format!("{change_id}-commit"),
            bookmarks: Vec::new(),
            description: format!("Change {change_id}"),
            description_body: String::new(),
            author: String::new(),
            stats: String::new(),
            commit_count: 1,
            commit_ids: vec![format!("{change_id}-commit")],
            change_ids: vec![change_id.to_string()],
            recent_log: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn ordered_form(head: &str, base: &str) -> PrForm {
        let mut status = StatusStore::new();
        status.set_revsets(Revsets::Loaded(vec![
            revset("new"),
            revset("base"),
            revset("old"),
        ]));
        let mut form = PrForm::default();
        form.sync_options(&status);
        form.head.set_value(head.to_string());
        form.base.set_value(base.to_string());
        form
    }

    #[test]
    fn base_picker_includes_default_base_as_oldest_option() {
        let mut status = StatusStore::new();
        status.set_revsets(Revsets::Loaded(vec![revset("new"), revset("old")]));
        let mut form = PrForm::new("main@origin".into());
        form.sync_options(&status);
        let FieldState::Picker(base) = &form.base else {
            panic!("base is a picker");
        };
        let values: Vec<&str> = base.options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, ["new", "old", "main@origin"]);
        // The head picker stays scoped to the changes — no trunk boundary.
        let FieldState::Picker(head) = &form.head else {
            panic!("head is a picker");
        };
        let head_values: Vec<&str> = head.options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(head_values, ["new", "old"]);
    }

    #[test]
    fn text_commit_and_cancel_track_value_and_dirty() {
        let mut t = TextFieldState::new("old".into(), false);
        t.begin_edit();
        t.input(key(KeyCode::End));
        t.input(key(KeyCode::Char('!')));
        t.commit();
        assert_eq!(t.value, "old!");
        assert!(t.dirty);
        t.begin_edit();
        t.input(key(KeyCode::Char('?')));
        t.cancel();
        assert_eq!(t.value, "old!");
        assert_eq!(t.buffer, "old!");
    }

    #[test]
    fn multiline_text_accepts_enter_before_commit() {
        let mut t = TextFieldState::new("one".into(), true);
        t.begin_edit();
        t.input(key(KeyCode::End));
        t.input(key(KeyCode::Enter));
        t.input(key(KeyCode::Char('2')));
        t.commit();
        assert_eq!(t.value, "one\n2");
    }

    #[test]
    fn picker_filters_and_toggles_multi_select() {
        let mut p = PickerFieldState::new(
            Vec::new(),
            true,
            true,
            vec![
                PickerOption {
                    label: "bug".into(),
                    value: "bug".into(),
                    enabled: true,
                },
                PickerOption {
                    label: "feature".into(),
                    value: "feature".into(),
                    enabled: true,
                },
            ],
        );
        p.begin_edit();
        p.input_filter(key(KeyCode::Char('f')));
        assert_eq!(p.visible_options()[0].value, "feature");
        p.toggle_highlighted();
        p.commit();
        assert_eq!(p.value, "feature");
    }

    #[test]
    fn picker_single_select_enter_commits_highlighted() {
        let mut p = PickerFieldState::new(
            Vec::new(),
            false,
            false,
            vec![
                PickerOption {
                    label: "main".into(),
                    value: "main".into(),
                    enabled: true,
                },
                PickerOption {
                    label: "trunk".into(),
                    value: "trunk".into(),
                    enabled: true,
                },
            ],
        );
        p.begin_edit();
        p.move_highlight(1);
        p.commit();
        assert_eq!(p.value, "trunk");
    }

    #[test]
    fn form_validation_flags_required_fields() {
        let mut form = PrForm::default();
        assert!(!form.validate());
        assert_eq!(
            form.field(FieldId::Head).errors(),
            &["required".to_string()]
        );
        form.head.set_value("abcd".into());
        form.branch_name.set_value("branch".into());
        form.base.set_value("main".into());
        form.title.set_value("Title".into());
        assert!(form.validate());
    }

    #[test]
    fn form_warns_when_head_is_older_than_base() {
        let form = ordered_form("old", "base");

        assert_eq!(
            form.relative_order_warning(FieldId::Head),
            Some(HEAD_BEHIND_BASE_WARNING)
        );
        assert_eq!(
            form.relative_order_warning(FieldId::Base),
            Some(BASE_AHEAD_OF_HEAD_WARNING)
        );
    }

    #[test]
    fn undo_restores_the_form_before_an_edit() {
        let mut form = PrForm::default();
        form.edit(|f| f.title.set_value("manual hints".into()));
        // The LLM overwrites several fields at once.
        form.edit(|f| {
            f.title.set_value("LLM title".into());
            f.description.set_value("LLM body".into());
        });
        assert_eq!(form.title(), "LLM title");

        // One whole-form undo reverts the entire overwrite.
        assert!(form.undo());
        assert_eq!(form.title(), "manual hints");
        assert_eq!(form.description(), "");

        // Redo reapplies it.
        assert!(form.redo());
        assert_eq!(form.title(), "LLM title");
        assert_eq!(form.description(), "LLM body");
    }

    #[test]
    fn undo_field_reverts_only_the_highlighted_field() {
        let mut form = PrForm::default();
        form.edit(|f| {
            f.title.set_value("manual title".into());
            f.description.set_value("manual body".into());
        });
        form.edit(|f| {
            f.title.set_value("llm title".into());
            f.description.set_value("llm body".into());
        });

        assert!(form.undo_field(FieldId::Title));
        assert_eq!(form.title(), "manual title");
        // The other field the same edit touched is left alone.
        assert_eq!(form.description(), "llm body");

        assert!(form.redo_field(FieldId::Title));
        assert_eq!(form.title(), "llm title");
    }

    #[test]
    fn undo_redo_report_nothing_to_do() {
        let mut form = PrForm::default();
        assert!(!form.undo());
        assert!(!form.redo());
        assert!(!form.undo_field(FieldId::Title));
        assert!(!form.redo_field(FieldId::Title));

        // A no-op edit records no history.
        form.edit(|_| {});
        assert!(!form.undo());
    }

    #[test]
    fn a_fresh_edit_clears_the_redo_tail() {
        let mut form = PrForm::default();
        form.edit(|f| f.title.set_value("one".into()));
        assert!(form.undo());
        assert_eq!(form.title(), "");

        // Editing again instead of redoing drops the pending redo.
        form.edit(|f| f.title.set_value("two".into()));
        assert!(!form.redo());
        assert_eq!(form.title(), "two");
    }

    #[test]
    fn picker_options_warn_against_counterpart_order() {
        let form = ordered_form("base", "base");

        assert_eq!(
            form.picker_option_warning(FieldId::Head, "old"),
            Some(HEAD_BEHIND_BASE_WARNING)
        );
        assert_eq!(form.picker_option_warning(FieldId::Head, "new"), None);
        assert_eq!(
            form.picker_option_warning(FieldId::Base, "new"),
            Some(BASE_AHEAD_OF_HEAD_WARNING)
        );
        assert_eq!(form.picker_option_warning(FieldId::Base, "old"), None);
    }
}
