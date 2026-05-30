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
    EditKey(crossterm::event::KeyEvent),
    Generate,
    ConfirmExecution,
    ExecuteConfirmed,
    TogglePromptView,
    Refresh,
    OpenCommentModal,
    SubmitComment,
    CancelComment,
    OpenPrInBrowser,
    CopyPrUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}
