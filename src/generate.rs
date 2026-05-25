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

#[derive(Debug, Clone)]
pub struct RevsetSummary {
    label: String,
    description: String,
    bookmarks: Vec<String>,
    stats: String,
}

impl RevsetSummary {
    pub fn new(label: &str, description: &str, bookmarks: &[&str], stats: &str) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            bookmarks: bookmarks
                .iter()
                .map(|bookmark| (*bookmark).into())
                .collect(),
            stats: stats.into(),
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
    pub fn demo() -> Self {
        let revsets = vec![
            RevsetSummary::new(
                "@",
                "Current working copy change",
                &["teatui-ui"],
                "3 files changed, +142 -12",
            ),
            RevsetSummary::new(
                "heads(trunk()..)",
                "Current stack above trunk",
                &[],
                "8 files changed, +426 -38",
            ),
            RevsetSummary::new("@-", "Parent change", &["main@origin"], "clean baseline"),
        ];
        let form = PrForm::new(
            revsets[0].label(),
            revsets[0].bookmarks().first().cloned().unwrap_or_default(),
            "main@origin",
        );

        Self {
            phase: GeneratePhase::SelectingRevset,
            selected_revset: 0,
            selected_field: 0,
            revsets,
            form,
            draft: None,
            review: DraftReview::default(),
        }
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
    }

    pub fn move_revset_down(&mut self) {
        self.selected_revset = (self.selected_revset + 1).min(self.revsets.len().saturating_sub(1));
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
}
