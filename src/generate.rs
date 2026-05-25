use std::time::SystemTime;

use crate::context::ContextBundle;
use crate::prompt::{DEFAULT_PROMPT_BYTE_BUDGET, PromptBuild};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Menu,
    Form,
    Preview,
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
    Confirming,
    Executing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptView {
    #[default]
    Manifest,
    Prompt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldState {
    initial: String,
    pub value: String,
    pub buffer: String,
    pub dirty: bool,
    pub errors: Vec<String>,
}

impl FieldState {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            initial: value.clone(),
            buffer: value.clone(),
            value,
            dirty: false,
            errors: Vec::new(),
        }
    }

    pub fn display_value(&self) -> &str {
        &self.buffer
    }

    pub fn begin_edit(&mut self) {
        self.buffer.clone_from(&self.value);
    }

    pub fn insert(&mut self, ch: char) {
        self.buffer.push(ch);
        self.dirty = self.buffer != self.initial;
    }

    pub fn backspace(&mut self) {
        self.buffer.pop();
        self.dirty = self.buffer != self.initial;
    }

    pub fn commit(&mut self) {
        if self.value != self.buffer {
            self.value.clone_from(&self.buffer);
        }
        self.dirty = self.value != self.initial;
    }

    pub fn cancel(&mut self) {
        self.buffer.clone_from(&self.value);
        self.dirty = self.value != self.initial;
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
            head: FieldState::new(head),
            branch_name: FieldState::new(branch_name),
            base: FieldState::new(base),
            title: FieldState::default(),
            description: FieldState::default(),
            labels: FieldState::default(),
            assignees: FieldState::default(),
            milestone: FieldState::default(),
        }
    }
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedDraft {
    pub branch_name: String,
    pub title: String,
    pub body: String,
    pub review_notes: Vec<String>,
    pub raw_model_response: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftReview {
    pub summary: String,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevsetSummary {
    label: String,
    description: String,
    bookmarks: Vec<String>,
    stats: String,
    commit_count: usize,
    commit_ids: Vec<String>,
    change_ids: Vec<String>,
    recent_log: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevsetUpdate {
    pub revsets: Vec<RevsetSummary>,
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
            bookmarks,
            stats: stats.into(),
            commit_count,
            commit_ids,
            change_ids,
            recent_log,
            warnings,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> &str {
        &self.description
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

impl RevsetUpdate {
    pub fn new(revsets: Vec<RevsetSummary>) -> Self {
        Self { revsets }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateState {
    pub phase: GeneratePhase,
    pub selected_revset: usize,
    pub selected_field: usize,
    pub revsets: Vec<RevsetSummary>,
    pub form: PrForm,
    pub context: Option<ContextBundle>,
    pub context_started_at: Option<SystemTime>,
    pub context_error: Option<String>,
    pub generation_error: Option<String>,
    pub draft: Option<GeneratedDraft>,
    pub review: DraftReview,
    pub prompt_view: PromptView,
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

        let mut state = Self {
            phase: GeneratePhase::SelectingRevset,
            selected_revset: 0,
            selected_field: 0,
            revsets,
            form: PrForm::new(default_head, default_branch, "main@origin"),
            context: None,
            context_started_at: None,
            context_error: None,
            generation_error: None,
            draft: None,
            review: DraftReview::default(),
            prompt_view: PromptView::default(),
        };
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

    pub fn begin_editing_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).begin_edit();
    }

    pub fn insert_into_selected_field(&mut self, ch: char) {
        self.form.field_mut(self.selected_field()).insert(ch);
        self.validate_form();
    }

    pub fn backspace_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).backspace();
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
        self.sync_head_from_selected_revset();
        self.validate_form();
    }

    pub fn sync_head_from_selected_revset(&mut self) {
        let selected = self.selected_revset().label().to_string();
        if !self.form.head.dirty {
            self.form.head = FieldState::new(selected);
        }
        if !self.form.branch_name.dirty {
            let branch_name = self
                .selected_revset()
                .bookmarks()
                .first()
                .cloned()
                .unwrap_or_default();
            self.form.branch_name = FieldState::new(branch_name);
        }
        self.validate_form();
    }

    pub fn validate_form(&mut self) {
        self.form.head.errors = required_field_errors("head", self.form.head.display_value());
        self.form.branch_name.errors = validate_branch_name(self.form.branch_name.display_value());
        self.form.base.errors = required_field_errors("base", self.form.base.display_value());
        self.form.title.errors.clear();
        self.form.description.errors.clear();
        self.form.labels.errors.clear();
        self.form.assignees.errors.clear();
        self.form.milestone.errors.clear();
    }

    pub fn blocking_errors(&self) -> Vec<String> {
        [&self.form.head, &self.form.branch_name, &self.form.base]
            .into_iter()
            .flat_map(|field| field.errors.iter().cloned())
            .collect()
    }

    pub fn begin_context_collection(&mut self) {
        self.phase = GeneratePhase::CollectingContext;
        self.context_started_at = Some(SystemTime::now());
        self.context_error = None;
        self.generation_error = None;
        self.context = None;
        self.draft = None;
        self.review = DraftReview::default();
    }

    pub fn complete_context_collection(&mut self, context: ContextBundle) {
        self.phase = GeneratePhase::ContextReady;
        self.context_started_at = Some(context.repo_identity.collected_at);
        self.context_error = None;
        self.generation_error = None;
        self.context = Some(context);
    }

    pub fn fail_context_collection(&mut self, error: impl Into<String>) {
        self.phase = GeneratePhase::Failed;
        self.context_error = Some(error.into());
        self.generation_error = None;
    }

    pub fn begin_generation(&mut self) {
        self.phase = GeneratePhase::Generating;
        self.context_error = None;
        self.generation_error = None;
        self.draft = None;
        self.review = DraftReview::default();
    }

    pub fn complete_generation(&mut self, draft: GeneratedDraft) {
        self.phase = GeneratePhase::DraftReady;
        self.generation_error = None;
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
        self.review = DraftReview {
            summary: "Generation failed".into(),
            notes: Vec::new(),
            warnings: vec![error],
        };
    }

    pub fn toggle_prompt_view(&mut self) {
        self.prompt_view = match self.prompt_view {
            PromptView::Manifest => PromptView::Prompt,
            PromptView::Prompt => PromptView::Manifest,
        };
    }

    pub fn prompt_build(&self) -> Option<PromptBuild> {
        self.context
            .as_ref()
            .map(|context| PromptBuild::new(context, &self.form, None, DEFAULT_PROMPT_BYTE_BUDGET))
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

fn required_field_errors(label: &str, value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        vec![format!("{label} is required")]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn replace_revsets_preserves_selected_label_when_present() {
        let mut state = GenerateState::new(vec![revset("@"), revset("@-")]);
        state.selected_revset = 1;

        state.replace_revsets(vec![revset("@"), revset("@-"), revset("heads(trunk()..)")]);

        assert_eq!(state.selected_revset().label(), "@-");
    }

    #[test]
    fn field_commit_updates_value_and_dirty_state() {
        let mut field = FieldState::new("initial");
        field.begin_edit();
        field.insert('x');
        field.commit();

        assert_eq!(field.value, "initialx");
        assert_eq!(field.display_value(), "initialx");
        assert!(field.dirty);
    }

    #[test]
    fn field_cancel_restores_buffer_without_changing_value() {
        let mut field = FieldState::new("initial");
        field.begin_edit();
        field.insert('x');
        field.cancel();

        assert_eq!(field.value, "initial");
        assert_eq!(field.display_value(), "initial");
        assert!(!field.dirty);
    }

    #[test]
    fn branch_name_validation_rejects_spaces_and_uppercase() {
        assert!(validate_branch_name("feature/foo-bar").is_empty());
        assert!(!validate_branch_name("Feature Foo").is_empty());
        assert!(!validate_branch_name("feature/foo..bar").is_empty());
    }

    #[test]
    fn empty_title_does_not_block_generation() {
        let mut state = GenerateState::new(vec![revset("@")]);
        state.form.branch_name = FieldState::new("feature/example");
        state.validate_form();

        assert!(state.form.title.errors.is_empty());
        assert!(state.blocking_errors().is_empty());
    }
}
