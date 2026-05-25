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
    pub value: String,
    pub buffer: String,
    pub dirty: bool,
    pub errors: Vec<String>,
}

impl FieldState {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            buffer: value.clone(),
            value,
            dirty: false,
            errors: Vec::new(),
        }
    }

    pub fn display_value(&self) -> &str {
        if self.dirty {
            &self.buffer
        } else {
            &self.value
        }
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

pub const FORM_FIELDS: [&str; 8] = [
    "head",
    "branch name",
    "base",
    "title",
    "description",
    "labels",
    "assignees",
    "milestone",
];

#[derive(Debug, Clone)]
pub struct GenerateState {
    pub phase: GeneratePhase,
    pub input_mode: InputMode,
    pub focus: Focus,
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
            input_mode: InputMode::Normal,
            focus: Focus::Menu,
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

    pub fn selected_field_name(&self) -> &'static str {
        FORM_FIELDS[self.selected_field]
    }
}
