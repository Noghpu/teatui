use std::{collections::BTreeSet, time::SystemTime};

use crate::bookmark_naming;
use crate::config::Config;
use crate::context::ContextBundle;
use crate::jj::JjClient;
use crate::prompt::{DEFAULT_PROMPT_BYTE_BUDGET, PromptBuild};
use crate::repo::RepoState;
use crate::tea::{PrCreateArgs, TeaClient};
use ratatui_textarea::{CursorMove, TextArea};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
    Confirm,
}

impl InputMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Editing => "EDITING",
            Self::Confirm => "CONFIRM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Menu,
    Form,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollState {
    pub offset: usize,
}

impl ScrollState {
    pub fn clamp(&mut self, content_height: usize, viewport_height: usize) {
        let max_offset = content_height.saturating_sub(viewport_height);
        self.offset = self.offset.min(max_offset);
    }

    pub fn scroll_up(&mut self) {
        self.offset = self.offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.offset = self.offset.saturating_add(1);
    }

    pub fn ensure_visible(
        &mut self,
        start: usize,
        end: usize,
        content_height: usize,
        viewport_height: usize,
    ) {
        if viewport_height == 0 || content_height == 0 {
            self.offset = 0;
            return;
        }

        let end = end.max(start + 1);
        let viewport_end = self.offset.saturating_add(viewport_height);

        if start < self.offset {
            self.offset = start;
        } else if end > viewport_end {
            self.offset = end.saturating_sub(viewport_height);
        }

        self.clamp(content_height, viewport_height);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum GeneratePhase {
    #[default]
    SelectingRevset,
    EditingForm,
    CollectingContext,
    ContextReady,
    Generating,
    DraftReady,
    CheckingFreshness,
    Confirming,
    Executing,
    Complete,
    Failed,
}

impl GeneratePhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::SelectingRevset => "selecting",
            Self::EditingForm => "editing",
            Self::CollectingContext => "collecting",
            Self::ContextReady => "context-ready",
            Self::Generating => "generating",
            Self::DraftReady => "draft-ready",
            Self::CheckingFreshness => "checking-freshness",
            Self::Confirming => "confirming",
            Self::Executing => "executing",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptView {
    #[default]
    Manifest,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text { multiline: bool },
    Picker { multi_select: bool, optional: bool },
}

impl FieldKind {
    pub fn is_multiline(self) -> bool {
        matches!(self, Self::Text { multiline: true })
    }

    pub fn is_picker(self) -> bool {
        matches!(self, Self::Picker { .. })
    }

    pub fn is_multi_select(self) -> bool {
        matches!(
            self,
            Self::Picker {
                multi_select: true,
                ..
            }
        )
    }

    pub fn is_optional(self) -> bool {
        matches!(self, Self::Picker { optional: true, .. })
    }
}

#[derive(Debug, Clone)]
pub struct TextFieldState {
    initial: String,
    pub value: String,
    pub buffer: String,
    pub editor: TextArea<'static>,
    pub dirty: bool,
    pub errors: Vec<String>,
}

impl TextFieldState {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            initial: value.clone(),
            buffer: value.clone(),
            value: value.clone(),
            editor: textarea_from_text(&value),
            dirty: false,
            errors: Vec::new(),
        }
    }

    pub fn display_value(&self) -> &str {
        &self.buffer
    }

    pub fn begin_edit(&mut self) {
        self.buffer.clone_from(&self.value);
        self.editor = textarea_from_text(&self.value);
    }

    pub fn commit(&mut self) {
        self.buffer = textarea_to_text(&self.editor);
        if self.value != self.buffer {
            self.value.clone_from(&self.buffer);
        }
        self.dirty = self.value != self.initial;
    }

    pub fn cancel(&mut self) {
        self.buffer.clone_from(&self.value);
        self.editor = textarea_from_text(&self.value);
        self.dirty = self.value != self.initial;
    }

    pub fn reset_editor_viewport(&mut self) {
        self.editor = textarea_from_text(&self.buffer);
    }

    pub fn input(&mut self, key: crossterm::event::KeyEvent) {
        let input = ratatui_textarea::Input {
            key: match key.code {
                crossterm::event::KeyCode::Char(ch) => ratatui_textarea::Key::Char(ch),
                crossterm::event::KeyCode::Backspace => ratatui_textarea::Key::Backspace,
                crossterm::event::KeyCode::Enter => ratatui_textarea::Key::Enter,
                crossterm::event::KeyCode::Left => ratatui_textarea::Key::Left,
                crossterm::event::KeyCode::Right => ratatui_textarea::Key::Right,
                crossterm::event::KeyCode::Up => ratatui_textarea::Key::Up,
                crossterm::event::KeyCode::Down => ratatui_textarea::Key::Down,
                crossterm::event::KeyCode::Tab => ratatui_textarea::Key::Tab,
                crossterm::event::KeyCode::BackTab => ratatui_textarea::Key::Tab,
                crossterm::event::KeyCode::Delete => ratatui_textarea::Key::Delete,
                crossterm::event::KeyCode::Home => ratatui_textarea::Key::Home,
                crossterm::event::KeyCode::End => ratatui_textarea::Key::End,
                crossterm::event::KeyCode::PageUp => ratatui_textarea::Key::PageUp,
                crossterm::event::KeyCode::PageDown => ratatui_textarea::Key::PageDown,
                crossterm::event::KeyCode::Esc => ratatui_textarea::Key::Esc,
                crossterm::event::KeyCode::F(n) => ratatui_textarea::Key::F(n),
                _ => ratatui_textarea::Key::Null,
            },
            ctrl: key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL),
            alt: key.modifiers.contains(crossterm::event::KeyModifiers::ALT),
            shift: key
                .modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT)
                || key.code == crossterm::event::KeyCode::BackTab,
        };
        self.editor.input(input);
        self.buffer = textarea_to_text(&self.editor);
        self.dirty = self.buffer != self.initial;
    }
}

impl PartialEq for TextFieldState {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for TextFieldState {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOption {
    pub label: String,
    pub value: String,
    pub enabled: bool,
}

impl PickerOption {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            enabled: true,
        }
    }

    pub fn disabled(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOptionView {
    pub label: String,
    pub value: String,
    pub enabled: bool,
    pub selected: bool,
    pub highlighted: bool,
}

#[derive(Debug, Clone)]
pub struct PickerFieldState {
    initial: Vec<String>,
    committed: Vec<String>,
    draft: Vec<String>,
    pub value: String,
    pub buffer: String,
    pub dirty: bool,
    pub errors: Vec<String>,
    pub options: Vec<PickerOption>,
    pub filter: String,
    pub highlighted: usize,
    pub multi_select: bool,
    pub optional: bool,
    pub editing: bool,
}

impl PickerFieldState {
    pub fn new(value: impl Into<String>, multi_select: bool, optional: bool) -> Self {
        let value = value.into();
        let committed = parse_picker_values(&value, multi_select);
        Self {
            initial: committed.clone(),
            committed: committed.clone(),
            draft: committed.clone(),
            value: value.clone(),
            buffer: value,
            dirty: false,
            errors: Vec::new(),
            options: Vec::new(),
            filter: String::new(),
            highlighted: 0,
            multi_select,
            optional,
            editing: false,
        }
    }

    pub fn display_value(&self) -> &str {
        if self.editing {
            &self.buffer
        } else {
            &self.value
        }
    }

    pub fn begin_edit(&mut self) {
        self.editing = true;
        self.draft = self.committed.clone();
        self.filter.clear();
        self.highlighted = self
            .visible_options()
            .iter()
            .position(|option| option.selected && option.enabled)
            .or_else(|| {
                self.visible_options()
                    .iter()
                    .position(|option| option.enabled)
            })
            .unwrap_or(0);
        self.sync_buffer();
    }

    pub fn commit(&mut self) {
        if self.editing && !self.multi_select {
            self.select_highlighted();
        }
        self.committed = self.draft.clone();
        self.value = join_picker_values(&self.committed);
        self.buffer = self.value.clone();
        self.dirty = self.committed != self.initial;
        self.filter.clear();
        self.editing = false;
    }

    pub fn cancel(&mut self) {
        self.draft = self.committed.clone();
        self.buffer = self.value.clone();
        self.filter.clear();
        self.highlighted = 0;
        self.dirty = self.committed != self.initial;
        self.editing = false;
    }

    pub fn reset_editor_viewport(&mut self) {}

    pub fn input(&mut self, key: crossterm::event::KeyEvent) {
        if !self.editing {
            return;
        }

        match key.code {
            crossterm::event::KeyCode::Char(ch)
                if key.modifiers.is_empty() && ch != ' ' && !ch.is_control() =>
            {
                self.filter.push(ch);
                self.highlighted = self
                    .highlighted
                    .min(self.visible_options().len().saturating_sub(1));
                self.sync_buffer();
            }
            crossterm::event::KeyCode::Backspace => {
                self.filter.pop();
                self.highlighted = self
                    .highlighted
                    .min(self.visible_options().len().saturating_sub(1));
                self.sync_buffer();
            }
            crossterm::event::KeyCode::Up => self.move_highlight(-1),
            crossterm::event::KeyCode::Down => self.move_highlight(1),
            crossterm::event::KeyCode::Char(' ') if self.multi_select => {
                self.toggle_highlighted();
            }
            _ => {}
        }
    }

    pub fn set_options(&mut self, options: Vec<PickerOption>) {
        self.options = options;
        self.ensure_valid_selection();
        self.sync_buffer();
    }

    pub fn visible_options(&self) -> Vec<PickerOptionView> {
        let filter = self.filter.trim().to_lowercase();
        self.options
            .iter()
            .filter(|option| {
                filter.is_empty()
                    || option.label.to_lowercase().contains(&filter)
                    || option.value.to_lowercase().contains(&filter)
            })
            .enumerate()
            .map(|(index, option)| {
                let selected = self.draft.iter().any(|value| value == &option.value);
                PickerOptionView {
                    label: option.label.clone(),
                    value: option.value.clone(),
                    enabled: option.enabled,
                    selected,
                    highlighted: index == self.highlighted,
                }
            })
            .collect()
    }

    pub fn selected_values(&self) -> &[String] {
        &self.committed
    }

    pub fn is_valid_selection(&self) -> bool {
        if self.committed.is_empty() {
            return self.optional;
        }

        self.committed.iter().all(|value| {
            self.options
                .iter()
                .any(|option| option.enabled && option.value == *value)
        })
    }

    pub fn invalid_selection_error(&self, label: &str) -> Vec<String> {
        if self.options.is_empty() {
            if self.optional {
                Vec::new()
            } else {
                vec![format!("{label} has no available options")]
            }
        } else if self.is_valid_selection() {
            Vec::new()
        } else {
            vec![format!("{label} selection is unavailable")]
        }
    }

    fn move_highlight(&mut self, direction: isize) {
        let visible = self.visible_option_indices();
        if visible.is_empty() {
            self.highlighted = 0;
            self.sync_buffer();
            return;
        }

        let current = visible
            .get(self.highlighted.min(visible.len().saturating_sub(1)))
            .copied()
            .unwrap_or(visible[0]);
        let position = visible
            .iter()
            .position(|index| *index == current)
            .unwrap_or(0);
        let next = if direction < 0 {
            position.saturating_sub(1)
        } else {
            (position + 1).min(visible.len().saturating_sub(1))
        };
        self.highlighted = next;
        self.sync_buffer();
    }

    fn toggle_highlighted(&mut self) {
        let Some(index) = self.visible_option_indices().get(self.highlighted).copied() else {
            return;
        };
        let Some(option) = self.options.get(index) else {
            return;
        };
        if !option.enabled {
            return;
        }

        if self.multi_select {
            if let Some(pos) = self.draft.iter().position(|value| value == &option.value) {
                self.draft.remove(pos);
            } else {
                self.draft.push(option.value.clone());
            }
        } else {
            self.draft.clear();
            self.draft.push(option.value.clone());
        }

        self.sync_buffer();
    }

    fn select_highlighted(&mut self) {
        let Some(index) = self.visible_option_indices().get(self.highlighted).copied() else {
            return;
        };
        let Some(option) = self.options.get(index) else {
            return;
        };
        if !option.enabled {
            return;
        }

        self.draft.clear();
        self.draft.push(option.value.clone());
        self.sync_buffer();
    }

    fn ensure_valid_selection(&mut self) {
        if self.options.is_empty() {
            self.committed.clear();
            self.draft.clear();
            self.value.clear();
            self.buffer.clear();
            self.highlighted = 0;
            return;
        }

        let enabled_values = self
            .options
            .iter()
            .filter(|option| option.enabled)
            .map(|option| option.value.clone())
            .collect::<Vec<_>>();
        self.committed
            .retain(|value| enabled_values.iter().any(|candidate| candidate == value));
        self.draft = self.committed.clone();
        if self.committed.is_empty()
            && !self.optional
            && let Some(value) = enabled_values.first()
        {
            self.committed.push(value.clone());
            self.draft = self.committed.clone();
        }
        self.value = join_picker_values(&self.committed);
        self.buffer = self.value.clone();
        self.dirty = self.committed != self.initial;
        self.highlighted = 0;
    }

    fn visible_option_indices(&self) -> Vec<usize> {
        let filter = self.filter.trim().to_lowercase();
        self.options
            .iter()
            .enumerate()
            .filter(|(_, option)| {
                filter.is_empty()
                    || option.label.to_lowercase().contains(&filter)
                    || option.value.to_lowercase().contains(&filter)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn sync_buffer(&mut self) {
        self.buffer = join_picker_values(&self.draft);
    }
}

impl PartialEq for PickerFieldState {
    fn eq(&self, other: &Self) -> bool {
        self.committed == other.committed
            && self.multi_select == other.multi_select
            && self.optional == other.optional
    }
}

impl Eq for PickerFieldState {}

#[derive(Debug, Clone)]
pub enum FieldState {
    Text(Box<TextFieldState>),
    Picker(PickerFieldState),
}

impl FieldState {
    pub fn new(value: impl Into<String>) -> Self {
        Self::Text(Box::new(TextFieldState::new(value)))
    }

    pub fn picker(value: impl Into<String>, multi_select: bool, optional: bool) -> Self {
        Self::Picker(PickerFieldState::new(value, multi_select, optional))
    }

    pub fn display_value(&self) -> &str {
        match self {
            Self::Text(field) => field.display_value(),
            Self::Picker(field) => field.display_value(),
        }
    }

    pub fn begin_edit(&mut self) {
        match self {
            Self::Text(field) => field.begin_edit(),
            Self::Picker(field) => field.begin_edit(),
        }
    }

    pub fn commit(&mut self) {
        match self {
            Self::Text(field) => field.commit(),
            Self::Picker(field) => field.commit(),
        }
    }

    pub fn cancel(&mut self) {
        match self {
            Self::Text(field) => field.cancel(),
            Self::Picker(field) => field.cancel(),
        }
    }

    pub fn reset_editor_viewport(&mut self) {
        match self {
            Self::Text(field) => field.reset_editor_viewport(),
            Self::Picker(field) => field.reset_editor_viewport(),
        }
    }

    pub fn input(&mut self, key: crossterm::event::KeyEvent) {
        match self {
            Self::Text(field) => field.input(key),
            Self::Picker(field) => field.input(key),
        }
    }

    pub fn set_picker_options(&mut self, options: Vec<PickerOption>) {
        if let Self::Picker(field) = self {
            field.set_options(options);
        }
    }

    pub fn picker_options(&self) -> &[PickerOption] {
        match self {
            Self::Text(_) => &[],
            Self::Picker(field) => &field.options,
        }
    }

    pub fn picker_visible_options(&self) -> Vec<PickerOptionView> {
        match self {
            Self::Text(_) => Vec::new(),
            Self::Picker(field) => field.visible_options(),
        }
    }

    pub fn picker_filter(&self) -> Option<&str> {
        match self {
            Self::Text(_) => None,
            Self::Picker(field) => Some(&field.filter),
        }
    }

    pub fn picker_is_editing(&self) -> bool {
        matches!(self, Self::Picker(field) if field.editing)
    }

    pub fn picker_selected_values(&self) -> &[String] {
        match self {
            Self::Text(_) => &[],
            Self::Picker(field) => field.selected_values(),
        }
    }

    pub fn is_picker(&self) -> bool {
        matches!(self, Self::Picker(_))
    }

    pub fn text_editor(&self) -> Option<&TextArea<'static>> {
        match self {
            Self::Text(field) => Some(&field.editor),
            Self::Picker(_) => None,
        }
    }

    pub fn is_multiline(&self) -> bool {
        matches!(self, Self::Text(field) if field.buffer.lines().count() > 1)
    }

    pub fn dirty(&self) -> bool {
        match self {
            Self::Text(field) => field.dirty,
            Self::Picker(field) => field.dirty,
        }
    }

    pub fn errors(&self) -> &[String] {
        match self {
            Self::Text(field) => &field.errors,
            Self::Picker(field) => &field.errors,
        }
    }

    pub fn errors_mut(&mut self) -> &mut Vec<String> {
        match self {
            Self::Text(field) => &mut field.errors,
            Self::Picker(field) => &mut field.errors,
        }
    }

    pub fn set_errors(&mut self, errors: Vec<String>) {
        *self.errors_mut() = errors;
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        match self {
            Self::Text(field) => {
                field.value = value.clone();
                field.buffer = value.clone();
                field.initial = value;
                field.dirty = false;
                field.editor = textarea_from_text(&field.value);
            }
            Self::Picker(field) => {
                let committed = parse_picker_values(&value, field.multi_select);
                field.initial = committed.clone();
                field.committed = committed.clone();
                field.draft = committed;
                field.value = value.clone();
                field.buffer = value;
                field.dirty = false;
            }
        }
    }

    pub fn set_picker_selection(&mut self, values: Vec<String>) {
        if let Self::Picker(field) = self {
            field.initial = values.clone();
            field.committed = values.clone();
            field.draft = values.clone();
            field.value = join_picker_values(&values);
            field.buffer = field.value.clone();
            field.dirty = false;
        }
    }
}

impl PartialEq for FieldState {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(left), Self::Text(right)) => left == right,
            (Self::Picker(left), Self::Picker(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for FieldState {}

impl Default for FieldState {
    fn default() -> Self {
        Self::new("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrForm {
    pub head: FieldState,
    pub branch_name: FieldState,
    pub base: FieldState,
    pub title: FieldState,
    pub description: FieldState,
    pub labels: FieldState,
    pub assignees: FieldState,
    pub milestone: FieldState,
}

impl PrForm {
    pub fn new(
        head: impl Into<String>,
        branch_name: impl Into<String>,
        base: impl Into<String>,
    ) -> Self {
        Self {
            head: FieldState::picker(head, false, false),
            branch_name: FieldState::new(branch_name),
            base: FieldState::picker(base, false, false),
            title: FieldState::default(),
            description: FieldState::default(),
            labels: FieldState::picker("", true, true),
            assignees: FieldState::picker("", true, true),
            milestone: FieldState::picker("", false, true),
        }
    }
}

fn textarea_from_text(text: &str) -> TextArea<'static> {
    let lines = if text.is_empty() {
        vec![String::new()]
    } else {
        text.lines().map(|line| line.to_string()).collect()
    };
    let mut textarea = TextArea::new(lines);
    textarea.move_cursor(CursorMove::Bottom);
    textarea.move_cursor(CursorMove::End);
    textarea
}

fn textarea_to_text(textarea: &TextArea<'static>) -> String {
    textarea
        .lines()
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_picker_values(value: &str, multi_select: bool) -> Vec<String> {
    let mut values = Vec::new();
    let mut seen = BTreeSet::new();

    for value in value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if seen.insert(value.to_string()) {
            values.push(value.to_string());
        }
        if !multi_select {
            break;
        }
    }

    values
}

fn join_picker_values(values: &[String]) -> String {
    values.join(", ")
}

impl Default for PrForm {
    fn default() -> Self {
        Self::new("", "", "")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
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
    pub const ALL: [Self; 8] = [
        Self::Head,
        Self::BranchName,
        Self::Base,
        Self::Title,
        Self::Description,
        Self::Labels,
        Self::Assignees,
        Self::Milestone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::BranchName => "branch name",
            Self::Base => "base",
            Self::Title => "title",
            Self::Description => "description",
            Self::Labels => "labels",
            Self::Assignees => "assignees",
            Self::Milestone => "milestone",
        }
    }

    pub fn kind(self) -> FieldKind {
        match self {
            Self::Description => FieldKind::Text { multiline: true },
            Self::BranchName | Self::Title => FieldKind::Text { multiline: false },
            Self::Head => FieldKind::Picker {
                multi_select: false,
                optional: false,
            },
            Self::Base => FieldKind::Picker {
                multi_select: false,
                optional: false,
            },
            Self::Labels | Self::Assignees => FieldKind::Picker {
                multi_select: true,
                optional: true,
            },
            Self::Milestone => FieldKind::Picker {
                multi_select: false,
                optional: true,
            },
        }
    }
}

impl PrForm {
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

    pub fn editors_mut(&mut self) -> impl Iterator<Item = &mut FieldState> {
        [
            &mut self.head,
            &mut self.branch_name,
            &mut self.base,
            &mut self.title,
            &mut self.description,
            &mut self.labels,
            &mut self.assignees,
            &mut self.milestone,
        ]
        .into_iter()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedDraft {
    pub branch_name: String,
    pub title: String,
    pub body: String,
    pub review_notes: Vec<String>,
    pub raw_model_response: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub pr_url: Option<String>,
    pub plan: ExecutionPlan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftReview {
    pub summary: String,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionStep {
    pub label: String,
    pub command: crate::command::ExternalCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleCheckResult {
    Fresh,
    Stale { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevsetSummary {
    label: String,
    description: String,
    description_body: String,
    bookmarks: Vec<String>,
    stats: String,
    commit_count: usize,
    commit_ids: Vec<String>,
    change_ids: Vec<String>,
    recent_log: Vec<String>,
    warnings: Vec<String>,
}

impl RevsetSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        label: &str,
        description: &str,
        bookmarks: Vec<String>,
        stats: &str,
        commit_count: usize,
        commit_ids: Vec<String>,
        change_ids: Vec<String>,
        recent_log: Vec<String>,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            description_body: String::new(),
            bookmarks,
            stats: stats.into(),
            commit_count,
            commit_ids,
            change_ids,
            recent_log,
            warnings,
        }
    }

    pub fn with_description_body(mut self, body: impl Into<String>) -> Self {
        self.description_body = body.into();
        self
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn description_body(&self) -> &str {
        &self.description_body
    }

    /// Returns true if the body contains non-whitespace content that is not a
    /// jj placeholder like "(no description set)".
    pub fn is_meaningful_body(&self) -> bool {
        let body = self.description_body.trim();
        if body.is_empty() {
            return false;
        }
        !body.eq_ignore_ascii_case("(no description set)")
    }

    pub fn bookmarks(&self) -> &[String] {
        &self.bookmarks
    }

    pub fn stats(&self) -> &str {
        &self.stats
    }

    pub fn commit_count(&self) -> usize {
        self.commit_count
    }

    pub fn commit_ids(&self) -> &[String] {
        &self.commit_ids
    }

    pub fn change_ids(&self) -> &[String] {
        &self.change_ids
    }

    pub fn recent_log(&self) -> &[String] {
        &self.recent_log
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

#[derive(Debug, Clone)]
pub struct GenerateState {
    pub phase: GeneratePhase,
    pub selected_revset: usize,
    pub selected_field: usize,
    pub menu_scroll: ScrollState,
    pub form_scroll: ScrollState,
    pub preview_scroll: ScrollState,
    pub revsets: Vec<RevsetSummary>,
    pub form: PrForm,
    pub context: Option<ContextBundle>,
    pub context_started_at: Option<SystemTime>,
    pub context_error: Option<String>,
    pub generation_error: Option<String>,
    pub draft: Option<GeneratedDraft>,
    pub execution_plan: Option<ExecutionPlan>,
    pub confirmation_summary: Option<String>,
    pub freshness_result: Option<StaleCheckResult>,
    pub completion: Option<Completion>,
    pub execution_step: Option<usize>,
    pub execution_total: Option<usize>,
    pub execution_error: Option<String>,
    pub execution_failed_step: Option<usize>,
    pub review: DraftReview,
    pub prompt_view: PromptView,
    prompt_cache: Option<PromptBuild>,
}

impl GenerateState {
    pub fn new(revsets: Vec<RevsetSummary>) -> Self {
        let default_head = revsets
            .first()
            .map(|revset| revset.label().to_string())
            .unwrap_or_default();
        let default_branch = revsets
            .first()
            .and_then(|revset| revset.bookmarks().first().cloned())
            .unwrap_or_default();

        let form = PrForm::new(default_head.clone(), default_branch, "main@origin");
        let mut state = Self {
            phase: GeneratePhase::SelectingRevset,
            selected_revset: 0,
            selected_field: 0,
            menu_scroll: ScrollState::default(),
            form_scroll: ScrollState::default(),
            preview_scroll: ScrollState::default(),
            revsets,
            form,
            context: None,
            context_started_at: None,
            context_error: None,
            generation_error: None,
            draft: None,
            execution_plan: None,
            confirmation_summary: None,
            freshness_result: None,
            completion: None,
            execution_step: None,
            execution_total: None,
            execution_error: None,
            execution_failed_step: None,
            review: DraftReview::default(),
            prompt_view: PromptView::default(),
            prompt_cache: None,
        };
        state.refresh_picker_options();
        if !default_head.is_empty() {
            state.form.head.set_value(default_head);
        }
        state.validate_form();
        state
    }

    pub fn with_placeholder(warning: impl Into<String>) -> Self {
        let warning = warning.into();
        let description = warning.clone();
        Self::new(vec![RevsetSummary::new(
            "(no revsets)",
            &description,
            Vec::new(),
            "0 files changed, 0 insertions(+), 0 deletions(-)",
            0,
            Vec::new(),
            Vec::new(),
            vec![warning.clone()],
            vec![warning],
        )])
    }

    pub fn selected_revset(&self) -> &RevsetSummary {
        &self.revsets[self.selected_revset]
    }

    pub fn selected_field(&self) -> FieldId {
        FieldId::ALL[self.selected_field]
    }

    pub fn selected_field_name(&self) -> &'static str {
        self.selected_field().label()
    }

    pub fn move_revset_up(&mut self) {
        self.selected_revset = self.selected_revset.saturating_sub(1);
        self.sync_head_from_selected_revset();
    }

    pub fn move_revset_down(&mut self) {
        self.selected_revset = (self.selected_revset + 1).min(self.revsets.len().saturating_sub(1));
        self.sync_head_from_selected_revset();
    }

    pub fn move_field_up(&mut self) {
        self.selected_field = self.selected_field.saturating_sub(1);
    }

    pub fn move_field_down(&mut self) {
        self.selected_field = (self.selected_field + 1).min(FieldId::ALL.len().saturating_sub(1));
    }

    pub fn scroll_preview_up(&mut self) {
        self.preview_scroll.scroll_up();
    }

    pub fn scroll_preview_down(&mut self) {
        self.preview_scroll.scroll_down();
    }

    pub fn begin_editing_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).begin_edit();
    }

    pub fn input_selected_field(&mut self, key: crossterm::event::KeyEvent) {
        self.form.field_mut(self.selected_field()).input(key);
        self.validate_form();
    }

    pub fn commit_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).commit();
        self.validate_form();
    }

    pub fn cancel_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).cancel();
        self.validate_form();
    }

    pub fn replace_revsets(&mut self, revsets: Vec<RevsetSummary>) {
        let previous_label = self.selected_revset().label().to_string();
        self.revsets = if revsets.is_empty() {
            vec![RevsetSummary::new(
                "(no revsets)",
                "No jj revsets could be loaded",
                Vec::new(),
                "0 files changed, 0 insertions(+), 0 deletions(-)",
                0,
                Vec::new(),
                Vec::new(),
                vec!["No jj revsets could be loaded".into()],
                vec!["No jj revsets could be loaded".into()],
            )]
        } else {
            revsets
        };
        self.selected_revset = self
            .revsets
            .iter()
            .position(|revset| revset.label() == previous_label)
            .unwrap_or(0);
        self.refresh_picker_options();
        self.sync_head_from_selected_revset();
        self.validate_form();
    }

    pub fn sync_head_from_selected_revset(&mut self) {
        let selected = self.selected_revset().label().to_string();
        if !self.form.head.dirty() {
            self.form.head.set_value(selected);
        }
        if !self.form.branch_name.dirty() {
            let branch_name = self
                .selected_revset()
                .bookmarks()
                .first()
                .cloned()
                .unwrap_or_default();
            self.form.branch_name.set_value(branch_name);
        }
        self.validate_form();
    }

    pub fn validate_form(&mut self) {
        let head_errors = self.form_picker_errors(&self.form.head, "head");
        let base_errors = self.form_picker_errors(&self.form.base, "base");
        let branch_errors = validate_optional_branch_name(self.form.branch_name.display_value());

        self.form.head.set_errors(head_errors);
        self.form.branch_name.set_errors(branch_errors);
        self.form.base.set_errors(base_errors);
        self.form.title.set_errors(Vec::new());
        self.form.description.set_errors(Vec::new());
        self.form.labels.set_errors(Vec::new());
        self.form.assignees.set_errors(Vec::new());
        self.form.milestone.set_errors(Vec::new());
        self.refresh_prompt_cache();
    }

    fn refresh_prompt_cache(&mut self) {
        self.prompt_cache = self
            .context
            .as_ref()
            .map(|context| PromptBuild::new(context, &self.form, None, DEFAULT_PROMPT_BYTE_BUDGET));
    }

    pub fn blocking_errors(&self) -> Vec<String> {
        [&self.form.head, &self.form.branch_name, &self.form.base]
            .into_iter()
            .flat_map(|field| field.errors().iter().cloned())
            .collect()
    }

    pub fn begin_context_collection(&mut self) {
        self.phase = GeneratePhase::CollectingContext;
        self.context_started_at = Some(SystemTime::now());
        self.context_error = None;
        self.generation_error = None;
        self.context = None;
        self.prompt_cache = None;
        self.clear_completion_state();
        self.clear_confirmation_state();
    }

    pub fn complete_context_collection(&mut self, context: ContextBundle) {
        self.phase = GeneratePhase::ContextReady;
        self.context_started_at = Some(context.repo_identity.collected_at);
        self.context_error = None;
        self.generation_error = None;
        self.context = Some(context);
        self.refresh_prompt_cache();
        self.clear_completion_state();
        self.clear_confirmation_state();
    }

    pub fn fail_context_collection(&mut self, error: impl Into<String>) {
        self.phase = GeneratePhase::Failed;
        self.context_error = Some(error.into());
        self.generation_error = None;
        self.clear_completion_state();
        self.clear_confirmation_state();
    }

    pub fn begin_generation(&mut self) {
        self.phase = GeneratePhase::Generating;
        self.context_error = None;
        self.generation_error = None;
        self.clear_completion_state();
        self.clear_confirmation_state();
    }

    pub fn complete_generation(&mut self, draft: GeneratedDraft) {
        self.phase = GeneratePhase::DraftReady;
        self.context_error = None;
        self.generation_error = None;
        self.clear_completion_state();
        self.clear_confirmation_state();
        self.sync_form_from_draft(&draft);
        self.review = DraftReview {
            summary: format!("Generated draft for {}", draft.branch_name),
            notes: draft.review_notes.clone(),
            warnings: Vec::new(),
        };
        self.draft = Some(draft);
    }

    pub fn fail_generation(&mut self, error: impl Into<String>) {
        let error = error.into();
        self.phase = GeneratePhase::Failed;
        self.context_error = None;
        self.generation_error = Some(error.clone());
        self.clear_completion_state();
        self.clear_confirmation_state();
        if self.draft.is_none() {
            self.review = DraftReview {
                summary: "Generation failed".into(),
                notes: Vec::new(),
                warnings: vec![error],
            };
        }
    }

    pub fn toggle_prompt_view(&mut self) {
        self.prompt_view = match self.prompt_view {
            PromptView::Manifest => PromptView::Prompt,
            PromptView::Prompt => PromptView::Manifest,
        };
    }

    pub fn begin_confirmation_check(&mut self) {
        self.phase = GeneratePhase::CheckingFreshness;
        self.confirmation_summary = Some("validation passed".into());
        self.freshness_result = None;
        self.execution_plan = None;
        self.execution_step = None;
        self.execution_total = None;
        self.execution_error = None;
        self.execution_failed_step = None;
    }

    pub fn complete_confirmation(&mut self, plan: ExecutionPlan) {
        self.phase = GeneratePhase::Confirming;
        self.confirmation_summary = Some("validation passed".into());
        self.freshness_result = Some(StaleCheckResult::Fresh);
        self.execution_plan = Some(plan);
        self.execution_step = None;
        self.execution_total = None;
        self.execution_error = None;
        self.execution_failed_step = None;
    }

    pub fn fail_confirmation(&mut self, reason: impl Into<String>) {
        self.phase = GeneratePhase::Failed;
        self.confirmation_summary = Some("validation passed".into());
        self.freshness_result = Some(StaleCheckResult::Stale {
            reason: reason.into(),
        });
        self.execution_plan = None;
        self.execution_step = None;
        self.execution_total = None;
        self.execution_error = None;
        self.execution_failed_step = None;
    }

    pub fn cancel_confirmation(&mut self) {
        self.phase = GeneratePhase::DraftReady;
        self.clear_confirmation_state();
    }

    pub fn begin_execution(&mut self) {
        self.phase = GeneratePhase::Executing;
        self.execution_step = None;
        self.execution_total = None;
        self.execution_error = None;
        self.execution_failed_step = None;
        self.completion = None;
    }

    pub fn record_execution_step(&mut self, index: usize, total: usize) {
        self.execution_step = Some(index);
        self.execution_total = Some(total);
    }

    pub fn complete_execution(&mut self, pr_url: Option<String>, plan: ExecutionPlan) {
        self.phase = GeneratePhase::Complete;
        self.completion = Some(Completion { pr_url, plan });
        self.execution_step = None;
        self.execution_total = None;
        self.execution_error = None;
        self.execution_failed_step = None;
    }

    pub fn fail_execution(&mut self, failed_step: Option<usize>, message: impl Into<String>) {
        self.phase = GeneratePhase::Failed;
        self.execution_error = Some(message.into());
        self.execution_failed_step = failed_step;
        self.execution_step = None;
        self.execution_total = None;
        self.completion = None;
    }

    pub fn clear_completion_state(&mut self) {
        self.completion = None;
        self.execution_step = None;
        self.execution_total = None;
        self.execution_error = None;
        self.execution_failed_step = None;
    }

    pub fn prompt(&self) -> Option<&PromptBuild> {
        self.prompt_cache.as_ref()
    }

    pub fn clear_confirmation_state(&mut self) {
        self.execution_plan = None;
        self.confirmation_summary = None;
        self.freshness_result = None;
    }

    fn sync_form_from_draft(&mut self, draft: &GeneratedDraft) {
        if !self.form.branch_name.dirty() {
            self.form.branch_name.set_value(draft.branch_name.clone());
        }
        if !self.form.title.dirty() {
            self.form.title.set_value(draft.title.clone());
        }
        if !self.form.description.dirty() {
            self.form.description.set_value(draft.body.clone());
        }
        self.validate_form();
    }

    fn refresh_picker_options(&mut self) {
        self.form
            .head
            .set_picker_options(self.head_picker_options());
        self.form
            .base
            .set_picker_options(self.base_picker_options());
        self.form.labels.set_picker_options(Vec::new());
        self.form.assignees.set_picker_options(Vec::new());
        self.form.milestone.set_picker_options(Vec::new());
    }

    fn head_picker_options(&self) -> Vec<PickerOption> {
        let mut options = Vec::new();
        for revset in &self.revsets {
            options.push(PickerOption::new(revset.label(), revset.label()));
        }
        options
    }

    fn base_picker_options(&self) -> Vec<PickerOption> {
        let mut options = Vec::new();
        let mut seen = BTreeSet::new();

        for revset in &self.revsets {
            for change_id in revset.change_ids() {
                if seen.insert(change_id.clone()) {
                    options.push(PickerOption::new(change_id.clone(), change_id.clone()));
                }
            }
        }

        if seen.insert("main@origin".into()) {
            options.push(PickerOption::new("main@origin", "main@origin"));
        }

        options
    }

    fn form_picker_errors(&self, field: &FieldState, label: &str) -> Vec<String> {
        match field {
            FieldState::Picker(picker) => picker.invalid_selection_error(label),
            FieldState::Text(_) => Vec::new(),
        }
    }
}

pub fn validate_branch_name(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.is_empty() {
        return vec!["branch name is required".into()];
    }

    let shape_ok = value.split('/').all(|segment| {
        !segment.is_empty()
            && !segment.starts_with('-')
            && !segment.ends_with('-')
            && segment
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    }) && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("//")
        && !value.contains("..")
        && !value.contains("@{")
        && !value.contains(".lock")
        && !value.starts_with('-')
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.chars().any(char::is_whitespace)
        && !value
            .chars()
            .any(|ch| matches!(ch, '\\' | '~' | '^' | ':' | '?' | '*' | '[' | ']'));

    if shape_ok {
        Vec::new()
    } else {
        vec!["branch name should use lowercase words separated by hyphens or slashes".into()]
    }
}

fn validate_optional_branch_name(value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        Vec::new()
    } else {
        validate_branch_name(value)
    }
}

pub fn validate_for_execution(form: &PrForm, _repo: &RepoState) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    push_required_error(&mut errors, "head", form.head.display_value());
    push_required_error(&mut errors, "base", form.base.display_value());
    push_required_error(&mut errors, "title", form.title.display_value());
    push_required_error(&mut errors, "body", form.description.display_value());

    let branch_name = form.branch_name.display_value().trim();
    if branch_name.is_empty() {
        errors.push("branch name is required".into());
    } else {
        errors.extend(
            validate_branch_name(branch_name)
                .into_iter()
                .map(|message| format!("branch name: {message}")),
        );
    }

    // Validate base vs head: if base looks like a change_id it must not equal
    // the head change_id (tip).
    let base = form.base.display_value().trim();
    let head = form.head.display_value().trim();
    if bookmark_naming::is_change_id_like(base)
        && bookmark_naming::is_change_id_like(head)
        && base == head
    {
        errors.push("base and head must be different changes".into());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

impl ExecutionPlan {
    pub fn from_draft(
        form: &PrForm,
        repo: &RepoState,
        revset: &RevsetSummary,
        config: &Config,
    ) -> Self {
        let cwd = repo
            .workspace_root
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let head = form.head.display_value().trim().to_string();

        // Determine the tip bookmark name.  If the user has not edited the
        // branch_name field it may be empty (no existing bookmark on the
        // change).  In that case auto-generate a deterministic name.
        let branch_name_raw = form.branch_name.display_value().trim().to_string();
        let tip_bookmark = if branch_name_raw.is_empty() {
            bookmark_naming::tip_bookmark(form.title.display_value(), &head)
        } else {
            branch_name_raw.clone()
        };

        // Determine the base.  An empty base field falls back to the repo's
        // configured base branch (typically `main` or `main@origin`).
        let base_raw = if form.base.display_value().trim().is_empty() {
            repo.base_branch.name.trim().to_string()
        } else {
            form.base.display_value().trim().to_string()
        };

        let jj = JjClient::new(config);
        let tea = TeaClient::new(config);

        // Step 1: create or move tip bookmark.
        let tip_bookmark_cmd = if revset
            .bookmarks()
            .iter()
            .any(|bookmark| bookmark == &tip_bookmark)
        {
            jj.bookmark_move_command(&cwd, &tip_bookmark, &head)
        } else {
            jj.bookmark_create_command(&cwd, &tip_bookmark, &head)
        };

        let mut steps = vec![
            ExecutionStep {
                label: "create or move bookmark".into(),
                command: tip_bookmark_cmd,
            },
            ExecutionStep {
                label: "push bookmark to origin".into(),
                command: jj.git_push_bookmark_command(&cwd, &tip_bookmark),
            },
        ];

        // Steps 3-4: if the base looks like a change_id, generate and push a
        // deterministic base bookmark.  Otherwise the base is already a remote
        // ref (e.g. `main@origin`) and no extra steps are needed.
        //
        // Note: this always uses `jj bookmark create` — if a bookmark already
        // exists on the base change jj will report an error.  Reusing an
        // existing bookmark requires a per-change revsets list (see
        // bookmark_naming module docs); deferred until the per-change left
        // column ticket lands.
        let pr_base_arg = if bookmark_naming::is_change_id_like(&base_raw) {
            let base_bookmark_name = bookmark_naming::base_bookmark(&tip_bookmark);

            let base_bookmark_cmd =
                jj.bookmark_create_command(&cwd, &base_bookmark_name, &base_raw);

            steps.push(ExecutionStep {
                label: "create base bookmark".into(),
                command: base_bookmark_cmd,
            });
            steps.push(ExecutionStep {
                label: "push base bookmark to origin".into(),
                command: jj.git_push_bookmark_command(&cwd, &base_bookmark_name),
            });

            base_bookmark_name
        } else {
            base_raw
        };

        steps.push(ExecutionStep {
            label: "create gitea PR".into(),
            command: tea.pr_create_command(
                &cwd,
                PrCreateArgs {
                    title: form.title.display_value(),
                    body: form.description.display_value(),
                    base: &pr_base_arg,
                    head: &tip_bookmark,
                    labels: form.labels.display_value(),
                    assignees: form.assignees.display_value(),
                    milestone: form.milestone.display_value(),
                },
            ),
        });

        Self { steps }
    }
}

fn push_required_error(errors: &mut Vec<String>, label: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{label} is required"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{
        BaseBranchInfo, BaseBranchSource, LlmBackendStatus, LlmStatus, RemoteInfo, RepoState,
        TeaAuth,
    };
    use std::path::PathBuf;

    fn revset(label: &str) -> RevsetSummary {
        RevsetSummary::new(
            label,
            "description",
            Vec::new(),
            "1 file changed",
            1,
            vec!["commit".into()],
            vec!["change".into()],
            vec!["commit change description".into()],
            Vec::new(),
        )
    }

    fn repo_state() -> RepoState {
        RepoState {
            workspace_root: Some(PathBuf::from("C:/repo")),
            inside_workspace: true,
            discovering: false,
            jj: crate::repo::ToolStatus::Available,
            git: crate::repo::ToolStatus::Available,
            tea: crate::repo::ToolStatus::Available,
            tea_auth: TeaAuth::Configured {
                host: "code.example.com".into(),
                user: Some("alice".into()),
            },
            remote: Some(RemoteInfo::parse("git@code.example.com:team/project.git")),
            base_branch: BaseBranchInfo {
                name: "main".into(),
                source: BaseBranchSource::Config,
            },
            llm_active: "default".into(),
            llm_backends: vec![LlmBackendStatus {
                name: "default".into(),
                backend_type: "ollama".into(),
                base_url: "http://localhost:11434".into(),
                model: "qwen2.5-coder:latest".into(),
                status: LlmStatus::Reachable,
            }],
            blockers: Vec::new(),
        }
    }

    #[test]
    fn replace_revsets_preserves_selected_label_when_present() {
        let mut state = GenerateState::new(vec![revset("@"), revset("@-")]);
        state.selected_revset = 1;

        state.replace_revsets(vec![revset("@"), revset("@-"), revset("heads(trunk()..)")]);

        assert_eq!(state.selected_revset().label(), "@-");
    }

    #[test]
    fn scroll_state_keeps_the_selected_range_visible() {
        let mut scroll = ScrollState { offset: 8 };

        scroll.ensure_visible(2, 4, 20, 6);
        assert_eq!(scroll.offset, 2);

        scroll.ensure_visible(15, 18, 20, 6);
        assert_eq!(scroll.offset, 12);

        scroll.clamp(8, 6);
        assert_eq!(scroll.offset, 2);
    }

    #[test]
    fn field_kind_distinguishes_multiline_description() {
        assert!(FieldId::Description.kind().is_multiline());
        assert!(!FieldId::Head.kind().is_multiline());
        assert!(!FieldId::Title.kind().is_multiline());
    }

    #[test]
    fn field_commit_updates_value_and_dirty_state() {
        let mut field = FieldState::new("initial");
        field.begin_edit();
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        ));
        field.commit();

        assert_eq!(field.display_value(), "initialx");
        assert!(field.dirty());
    }

    #[test]
    fn field_cancel_restores_buffer_without_changing_value() {
        let mut field = FieldState::new("initial");
        field.begin_edit();
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        ));
        field.cancel();

        assert_eq!(field.display_value(), "initial");
        assert!(!field.dirty());
    }

    #[test]
    fn field_editing_supports_multiline_input() {
        let mut field = FieldState::new("initial");
        field.begin_edit();
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        ));
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        ));
        field.commit();

        assert_eq!(field.display_value(), "initial\nx");
        assert!(field.dirty());
    }

    #[test]
    fn field_cancel_restores_multiline_editor_state() {
        let mut field = FieldState::new("initial");
        field.begin_edit();
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        ));
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        ));
        field.cancel();

        assert_eq!(field.display_value(), "initial");
        assert!(!field.dirty());
    }

    #[test]
    fn single_select_picker_commits_highlighted_option() {
        let mut field = FieldState::picker("alpha", false, false);
        field.set_picker_options(vec![
            PickerOption::new("Alpha", "alpha"),
            PickerOption::new("Beta", "beta"),
        ]);

        field.begin_edit();
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        field.commit();

        assert_eq!(field.display_value(), "beta");
        assert_eq!(field.picker_selected_values(), &["beta".to_string()]);
        assert!(field.dirty());
    }

    #[test]
    fn picker_filter_limits_visible_options_and_commit_selection() {
        let mut field = FieldState::picker("", false, false);
        field.set_picker_options(vec![
            PickerOption::new("main@origin", "main@origin"),
            PickerOption::new("release@origin", "release@origin"),
        ]);

        field.begin_edit();
        for ch in "rel".chars() {
            field.input(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ));
        }

        let visible = field.picker_visible_options();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].value, "release@origin");

        field.commit();

        assert_eq!(field.display_value(), "release@origin");
    }

    #[test]
    fn multi_select_picker_toggles_with_space_and_cancels_draft() {
        let mut field = FieldState::picker("", true, true);
        field.set_picker_options(vec![
            PickerOption::new("bug", "bug"),
            PickerOption::new("ui", "ui"),
        ]);

        field.begin_edit();
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::empty(),
        ));
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        field.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::empty(),
        ));

        assert_eq!(field.display_value(), "bug, ui");

        field.cancel();

        assert_eq!(field.display_value(), "");
        assert!(field.picker_selected_values().is_empty());
    }

    #[test]
    fn picker_options_drop_unavailable_committed_values() {
        let mut field = FieldState::picker("old", false, true);
        field.set_picker_options(Vec::new());

        assert_eq!(field.display_value(), "");
        assert!(field.picker_selected_values().is_empty());
    }

    #[test]
    fn branch_name_validation_rejects_spaces_and_uppercase() {
        assert!(validate_branch_name("feature/foo-bar").is_empty());
        assert!(!validate_branch_name("Feature Foo").is_empty());
        assert!(!validate_branch_name("feature/foo..bar").is_empty());
    }

    #[test]
    fn empty_branch_name_and_title_do_not_block_generation() {
        let mut state = GenerateState::new(vec![revset("@")]);
        state.validate_form();

        assert!(state.form.branch_name.errors().is_empty());
        assert!(state.form.title.errors().is_empty());
        assert!(state.blocking_errors().is_empty());
    }

    #[test]
    fn complete_generation_syncs_form_with_draft_fields() {
        let mut state = GenerateState::new(vec![revset("@")]);
        let draft = GeneratedDraft {
            branch_name: "feature/example".into(),
            title: "Polished draft".into(),
            body: "Summary\n\nTesting".into(),
            review_notes: vec!["keep an eye on truncation".into()],
            raw_model_response: "{\"branch_name\":\"feature/example\"}".into(),
        };

        state.complete_generation(draft.clone());

        assert_eq!(state.phase, GeneratePhase::DraftReady);
        assert_eq!(state.form.branch_name.display_value(), draft.branch_name);
        assert_eq!(state.form.title.display_value(), draft.title);
        assert_eq!(state.form.description.display_value(), draft.body);
        assert_eq!(state.review.summary, "Generated draft for feature/example");
        assert_eq!(state.review.notes, draft.review_notes);
        assert_eq!(state.draft, Some(draft));
    }

    #[test]
    fn complete_generation_preserves_user_edited_draft_fields() {
        let mut state = GenerateState::new(vec![revset("@")]);
        state.complete_generation(GeneratedDraft {
            branch_name: "feature/example".into(),
            title: "Polished draft".into(),
            body: "Summary".into(),
            review_notes: Vec::new(),
            raw_model_response: "{}".into(),
        });
        state.form.title.begin_edit();
        for ch in " edited".chars() {
            state.form.title.input(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ));
        }
        state.form.title.commit();
        state.begin_generation();

        let retry_draft = GeneratedDraft {
            branch_name: "feature/retry".into(),
            title: "Retry draft".into(),
            body: "Retry summary".into(),
            review_notes: Vec::new(),
            raw_model_response: "{}".into(),
        };
        state.complete_generation(retry_draft);

        assert_eq!(state.form.branch_name.display_value(), "feature/retry");
        assert_eq!(state.form.title.display_value(), "Polished draft edited");
        assert_eq!(state.form.description.display_value(), "Retry summary");
    }

    #[test]
    fn begin_generation_keeps_the_last_draft_available_for_retry() {
        let mut state = GenerateState::new(vec![revset("@")]);
        state.complete_generation(GeneratedDraft {
            branch_name: "feature/example".into(),
            title: "Polished draft".into(),
            body: "Summary".into(),
            review_notes: vec!["keep an eye on truncation".into()],
            raw_model_response: "{\"branch_name\":\"feature/example\"}".into(),
        });

        state.begin_generation();

        assert_eq!(state.phase, GeneratePhase::Generating);
        assert_eq!(
            state.draft.as_ref().map(|draft| draft.branch_name.as_str()),
            Some("feature/example")
        );
        assert_eq!(state.form.branch_name.display_value(), "feature/example");
        assert_eq!(state.form.title.display_value(), "Polished draft");
        assert_eq!(state.form.description.display_value(), "Summary");
    }

    #[test]
    fn begin_context_collection_keeps_the_last_draft_visible() {
        let mut state = GenerateState::new(vec![revset("@")]);
        let draft = GeneratedDraft {
            branch_name: "feature/example".into(),
            title: "Polished draft".into(),
            body: "Summary\n\nTesting".into(),
            review_notes: vec!["keep an eye on truncation".into()],
            raw_model_response: "{\"branch_name\":\"feature/example\"}".into(),
        };
        state.complete_generation(draft.clone());

        state.begin_context_collection();

        assert_eq!(state.phase, GeneratePhase::CollectingContext);
        assert_eq!(state.context, None);
        assert_eq!(state.draft, Some(draft));
        assert_eq!(state.review.summary, "Generated draft for feature/example");
    }

    #[test]
    fn validate_for_execution_rejects_bad_inputs() {
        let mut form = PrForm::new("", "", "");
        form.branch_name = FieldState::new("Feature Bad");
        form.labels = FieldState::new("bug;rm -rf");
        form.assignees = FieldState::new("alice");
        form.milestone = FieldState::new("v1");

        let errors = validate_for_execution(&form, &repo_state()).expect_err("errors");

        assert!(errors.iter().any(|error| error == "head is required"));
        assert!(errors.iter().any(|error| error == "base is required"));
        assert!(errors.iter().any(|error| error == "title is required"));
        assert!(errors.iter().any(|error| error == "body is required"));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("branch name: branch name should use lowercase words"))
        );
        assert!(!errors.iter().any(|error| error.contains("labels")));
    }

    #[test]
    fn execution_plan_creates_bookmark_when_missing() {
        let form = PrForm::new("@", "feature/example", "main");
        let mut form = form;
        form.title = FieldState::new("Create a PR");
        form.description = FieldState::new("Body");

        let config = crate::config::Config::default();
        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset("@"), &config);

        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].label, "create or move bookmark");
        assert_eq!(
            plan.steps[0].command.args,
            vec![
                "--no-pager",
                "bookmark",
                "create",
                "feature/example",
                "-r",
                "@"
            ]
        );
        assert_eq!(
            plan.steps[2].command.args,
            vec![
                "pr",
                "create",
                "--title",
                "Create a PR",
                "--description",
                "Body",
                "--base",
                "main",
                "--head",
                "feature/example"
            ]
        );
    }

    #[test]
    fn execution_plan_moves_existing_bookmark_when_present() {
        let mut form = PrForm::new("@", "feature/example", "main");
        form.title = FieldState::new("Create a PR");
        form.description = FieldState::new("Body");
        let revset = RevsetSummary::new(
            "@",
            "description",
            vec!["feature/example".into()],
            "1 file changed",
            1,
            vec!["commit".into()],
            vec!["change".into()],
            vec!["commit change description".into()],
            Vec::new(),
        );

        let config = crate::config::Config::default();
        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset, &config);

        assert_eq!(
            plan.steps[0].command.args,
            vec![
                "--no-pager",
                "bookmark",
                "move",
                "feature/example",
                "--to",
                "@"
            ]
        );
    }

    #[test]
    fn is_meaningful_body_empty_is_false() {
        let revset = revset("@");
        assert!(!revset.is_meaningful_body());
    }

    #[test]
    fn is_meaningful_body_whitespace_only_is_false() {
        let revset = revset("@").with_description_body("   \n  ");
        assert!(!revset.is_meaningful_body());
    }

    #[test]
    fn is_meaningful_body_placeholder_is_false() {
        let revset = revset("@").with_description_body("(no description set)");
        assert!(!revset.is_meaningful_body());
    }

    #[test]
    fn is_meaningful_body_placeholder_case_insensitive_is_false() {
        let revset = revset("@").with_description_body("(No Description Set)");
        assert!(!revset.is_meaningful_body());
    }

    #[test]
    fn is_meaningful_body_real_content_is_true() {
        let revset =
            revset("@").with_description_body("This is additional context for the change.");
        assert!(revset.is_meaningful_body());
    }

    #[test]
    fn execution_plan_base_as_remote_ref_produces_three_steps() {
        // Base is a remote ref (contains '@') — classic path, 3 steps.
        let mut form = PrForm::new("abcdefgh", "feature/tip", "main@origin");
        form.title = FieldState::new("My PR");
        form.description = FieldState::new("Body");

        let config = crate::config::Config::default();
        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset("abcdefgh"), &config);

        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].label, "create or move bookmark");
        assert_eq!(plan.steps[1].label, "push bookmark to origin");
        assert_eq!(plan.steps[2].label, "create gitea PR");
        // PR create uses literal remote ref as base.
        assert!(
            plan.steps[2]
                .command
                .args
                .contains(&"main@origin".to_string())
        );
        assert!(
            plan.steps[2]
                .command
                .args
                .contains(&"feature/tip".to_string())
        );
    }

    #[test]
    fn execution_plan_base_as_change_id_produces_five_steps() {
        // Base is a change_id — needs bookmark create+push for base.
        let tip_change = "abcdefgh";
        let base_change = "xyzuvwrs";
        let mut form = PrForm::new(tip_change, "feature/tip", base_change);
        form.title = FieldState::new("My PR");
        form.description = FieldState::new("Body");

        let config = crate::config::Config::default();
        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset(tip_change), &config);

        assert_eq!(plan.steps.len(), 5);
        assert_eq!(plan.steps[0].label, "create or move bookmark");
        assert_eq!(plan.steps[1].label, "push bookmark to origin");
        assert_eq!(plan.steps[2].label, "create base bookmark");
        assert_eq!(plan.steps[3].label, "push base bookmark to origin");
        assert_eq!(plan.steps[4].label, "create gitea PR");

        // Tip bookmark step: bookmark create at the tip change.
        assert!(plan.steps[0].command.args.contains(&tip_change.to_string()));
        // Base bookmark step: bookmark create at the base change.
        assert!(
            plan.steps[2]
                .command
                .args
                .contains(&base_change.to_string())
        );
        // Base push step references the auto-generated base bookmark name.
        let base_bm_name = &plan.steps[3].command.args;
        assert!(base_bm_name.iter().any(|a| a.starts_with("pr-base/")));
        // PR create uses the base bookmark name, not the raw change_id.
        let pr_args = &plan.steps[4].command.args;
        let base_idx = pr_args
            .iter()
            .position(|a| a == "--base")
            .expect("--base flag");
        let pr_base_arg = &pr_args[base_idx + 1];
        assert!(
            pr_base_arg.starts_with("pr-base/"),
            "pr create base should be bookmark name, got: {pr_base_arg}"
        );
    }

    #[test]
    fn execution_plan_auto_tip_bookmark_from_title_when_branch_name_empty() {
        // When branch_name is empty, tip bookmark is auto-generated from title.
        let mut form = PrForm::new("abcdefgh", "", "main");
        form.title = FieldState::new("Add login page");
        form.description = FieldState::new("Body");

        let config = crate::config::Config::default();
        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset("abcdefgh"), &config);

        assert_eq!(plan.steps.len(), 3);
        // Tip bookmark should be auto-generated from the title slug.
        let create_args = &plan.steps[0].command.args;
        assert!(
            create_args.iter().any(|a| a == "pr/add-login-page"),
            "auto-generated tip bookmark missing: {create_args:?}"
        );
    }

    #[test]
    fn validate_for_execution_rejects_base_equal_to_head_when_both_change_ids() {
        let change_id = "abcdefgh";
        let mut form = PrForm::new(change_id, "feature/tip", change_id);
        form.title = FieldState::new("Title");
        form.description = FieldState::new("Body");

        let errors = validate_for_execution(&form, &repo_state()).expect_err("expected errors");
        assert!(
            errors
                .iter()
                .any(|e| e.contains("base and head must be different")),
            "expected base==head error, got: {errors:?}"
        );
    }
}
