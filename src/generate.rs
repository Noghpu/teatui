use std::time::SystemTime;

use crate::command::ExternalCommand;
use crate::context::ContextBundle;
use crate::prompt::{DEFAULT_PROMPT_BYTE_BUDGET, PromptBuild};
use crate::repo::RepoState;

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
    pub execution_plan: Option<ExecutionPlan>,
    pub confirmation_summary: Option<String>,
    pub freshness_result: Option<StaleCheckResult>,
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
            execution_plan: None,
            confirmation_summary: None,
            freshness_result: None,
            review: DraftReview::default(),
            prompt_view: PromptView::default(),
            prompt_cache: None,
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
        self.form.branch_name.errors =
            validate_optional_branch_name(self.form.branch_name.display_value());
        self.form.base.errors = required_field_errors("base", self.form.base.display_value());
        self.form.title.errors.clear();
        self.form.description.errors.clear();
        self.form.labels.errors.clear();
        self.form.assignees.errors.clear();
        self.form.milestone.errors.clear();
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
            .flat_map(|field| field.errors.iter().cloned())
            .collect()
    }

    pub fn begin_context_collection(&mut self) {
        self.phase = GeneratePhase::CollectingContext;
        self.context_started_at = Some(SystemTime::now());
        self.context_error = None;
        self.generation_error = None;
        self.context = None;
        self.prompt_cache = None;
        self.clear_confirmation_state();
    }

    pub fn complete_context_collection(&mut self, context: ContextBundle) {
        self.phase = GeneratePhase::ContextReady;
        self.context_started_at = Some(context.repo_identity.collected_at);
        self.context_error = None;
        self.generation_error = None;
        self.context = Some(context);
        self.refresh_prompt_cache();
        self.clear_confirmation_state();
    }

    pub fn fail_context_collection(&mut self, error: impl Into<String>) {
        self.phase = GeneratePhase::Failed;
        self.context_error = Some(error.into());
        self.generation_error = None;
        self.clear_confirmation_state();
    }

    pub fn begin_generation(&mut self) {
        self.phase = GeneratePhase::Generating;
        self.context_error = None;
        self.generation_error = None;
        self.clear_confirmation_state();
    }

    pub fn complete_generation(&mut self, draft: GeneratedDraft) {
        self.phase = GeneratePhase::DraftReady;
        self.context_error = None;
        self.generation_error = None;
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
    }

    pub fn complete_confirmation(&mut self, plan: ExecutionPlan) {
        self.phase = GeneratePhase::Confirming;
        self.confirmation_summary = Some("validation passed".into());
        self.freshness_result = Some(StaleCheckResult::Fresh);
        self.execution_plan = Some(plan);
    }

    pub fn fail_confirmation(&mut self, reason: impl Into<String>) {
        self.phase = GeneratePhase::Failed;
        self.confirmation_summary = Some("validation passed".into());
        self.freshness_result = Some(StaleCheckResult::Stale {
            reason: reason.into(),
        });
        self.execution_plan = None;
    }

    pub fn cancel_confirmation(&mut self) {
        self.phase = GeneratePhase::DraftReady;
        self.clear_confirmation_state();
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
        if !self.form.branch_name.dirty {
            self.form.branch_name = FieldState::new(draft.branch_name.clone());
        }
        if !self.form.title.dirty {
            self.form.title = FieldState::new(draft.title.clone());
        }
        if !self.form.description.dirty {
            self.form.description = FieldState::new(draft.body.clone());
        }
        self.validate_form();
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

fn required_field_errors(label: &str, value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        vec![format!("{label} is required")]
    } else {
        Vec::new()
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

    errors.extend(validate_no_shell_metacharacters(
        "labels",
        form.labels.display_value(),
    ));
    errors.extend(validate_no_shell_metacharacters(
        "assignees",
        form.assignees.display_value(),
    ));
    errors.extend(validate_no_shell_metacharacters(
        "milestone",
        form.milestone.display_value(),
    ));

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

impl ExecutionPlan {
    pub fn from_draft(form: &PrForm, repo: &RepoState, revset: &RevsetSummary) -> Self {
        let cwd = repo
            .workspace_root
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let head = form.head.display_value().trim().to_string();
        let branch_name = form.branch_name.display_value().trim().to_string();
        let base = if form.base.display_value().trim().is_empty() {
            repo.base_branch.name.trim().to_string()
        } else {
            form.base.display_value().trim().to_string()
        };

        let bookmark_command = if revset
            .bookmarks()
            .iter()
            .any(|bookmark| bookmark == &branch_name)
        {
            ExternalCommand::new(
                "jj",
                [
                    "bookmark",
                    "move",
                    branch_name.as_str(),
                    "--to",
                    head.as_str(),
                ],
                &cwd,
            )
        } else {
            ExternalCommand::new(
                "jj",
                [
                    "bookmark",
                    "create",
                    branch_name.as_str(),
                    "-r",
                    head.as_str(),
                ],
                &cwd,
            )
        };

        let mut tea_args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--title".to_string(),
            form.title.display_value().trim().to_string(),
            "--description".to_string(),
            form.description.display_value().trim().to_string(),
            "--base".to_string(),
            base,
            "--head".to_string(),
            branch_name.clone(),
        ];

        for label in split_multi_values(form.labels.display_value()) {
            tea_args.push("--label".into());
            tea_args.push(label);
        }
        for assignee in split_multi_values(form.assignees.display_value()) {
            tea_args.push("--assignee".into());
            tea_args.push(assignee);
        }
        if let Some(milestone) = optional_single_value(form.milestone.display_value()) {
            tea_args.push("--milestone".into());
            tea_args.push(milestone);
        }

        Self {
            steps: vec![
                ExecutionStep {
                    label: "create or move bookmark".into(),
                    command: bookmark_command,
                },
                ExecutionStep {
                    label: "push bookmark to origin".into(),
                    command: ExternalCommand::new(
                        "jj",
                        ["git", "push", "--bookmark", branch_name.as_str()],
                        &cwd,
                    ),
                },
                ExecutionStep {
                    label: "create gitea PR".into(),
                    command: ExternalCommand::new("tea", tea_args, &cwd),
                },
            ],
        }
    }
}

fn push_required_error(errors: &mut Vec<String>, label: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{label} is required"));
    }
}

fn validate_no_shell_metacharacters(label: &str, value: &str) -> Vec<String> {
    if value.trim().is_empty() {
        return Vec::new();
    }

    if value.chars().any(is_shell_metacharacter) {
        vec![format!("{label} contains shell metacharacters")]
    } else {
        Vec::new()
    }
}

fn is_shell_metacharacter(ch: char) -> bool {
    matches!(ch, ';' | '&' | '|' | '`' | '$' | '<' | '>' | '\n' | '\r')
}

fn split_multi_values(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn optional_single_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{
        BaseBranchInfo, BaseBranchSource, OllamaStatus, RemoteInfo, RepoState, TeaAuth,
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
            ollama_base_url: "http://localhost:11434".into(),
            ollama_model: "qwen2.5-coder:latest".into(),
            ollama: OllamaStatus::Reachable,
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
    fn empty_branch_name_and_title_do_not_block_generation() {
        let mut state = GenerateState::new(vec![revset("@")]);
        state.validate_form();

        assert!(state.form.branch_name.errors.is_empty());
        assert!(state.form.title.errors.is_empty());
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
        assert_eq!(state.form.branch_name.value, draft.branch_name);
        assert_eq!(state.form.title.value, draft.title);
        assert_eq!(state.form.description.value, draft.body);
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
            state.form.title.insert(ch);
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

        assert_eq!(state.form.branch_name.value, "feature/retry");
        assert_eq!(state.form.title.value, "Polished draft edited");
        assert_eq!(state.form.description.value, "Retry summary");
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
        assert_eq!(state.form.branch_name.value, "feature/example");
        assert_eq!(state.form.title.value, "Polished draft");
        assert_eq!(state.form.description.value, "Summary");
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
        assert!(
            errors
                .iter()
                .any(|error| error == "labels contains shell metacharacters")
        );
    }

    #[test]
    fn execution_plan_creates_bookmark_when_missing() {
        let form = PrForm::new("@", "feature/example", "main");
        let mut form = form;
        form.title = FieldState::new("Create a PR");
        form.description = FieldState::new("Body");

        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset("@"));

        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].label, "create or move bookmark");
        assert_eq!(
            plan.steps[0].command.args,
            vec!["bookmark", "create", "feature/example", "-r", "@"]
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

        let plan = ExecutionPlan::from_draft(&form, &repo_state(), &revset);

        assert_eq!(
            plan.steps[0].command.args,
            vec!["bookmark", "move", "feature/example", "--to", "@"]
        );
    }
}
