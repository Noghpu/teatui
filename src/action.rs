#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Tick,
    Quit,
    Back,
    Navigate(Direction),
    FocusNext,
    FocusPrev,
    Select,
    Edit,
    InsertChar(char),
    Backspace,
    CommitEdit,
    CancelEdit,
    Generate,
    TogglePromptView,
    Refresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}
