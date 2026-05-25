#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Action {
    Tick,
    Render,
    Quit,
    Navigate(Direction),
    Focus(Direction),
    Select,
    Edit,
    InsertChar(char),
    Backspace,
    CommitEdit,
    CancelEdit,
    Generate,
    TogglePromptView,
    Refresh,
    Back,
    Context(Box<crate::context::ContextResult>),
    RepoUpdated(Box<crate::repo::RepoState>),
    RevsetsUpdated(Box<crate::generate::RevsetUpdate>),
    JobResult(crate::event::JobResult),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}
