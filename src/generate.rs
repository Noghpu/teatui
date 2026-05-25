#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
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
    ContextReady,
    Generating,
    DraftReady,
    Confirming,
    Executing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone, Default)]
pub struct GeneratedDraft {
    pub branch_name: String,
    pub title: String,
    pub body: String,
    pub review_notes: Vec<String>,
    pub raw_model_response: String,
}

#[derive(Debug, Clone, Default)]
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
    pub draft: Option<GeneratedDraft>,
    pub review: DraftReview,
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

        Self {
            phase: GeneratePhase::SelectingRevset,
            selected_revset: 0,
            selected_field: 0,
            revsets,
            form: PrForm::new(default_head, default_branch, "main@origin"),
            draft: None,
            review: DraftReview::default(),
        }
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
    }

    pub fn backspace_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).backspace();
    }

    pub fn commit_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).commit();
    }

    pub fn cancel_selected_field(&mut self) {
        self.form.field_mut(self.selected_field()).cancel();
    }

    pub fn replace_revsets(&mut self, revsets: Vec<RevsetSummary>) {
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
        self.selected_revset = 0;
        self.sync_head_from_selected_revset();
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
    }
}
