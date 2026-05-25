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
    Back,
    JobResult(crate::event::JobResult),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}
