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
    Generate,
    Back,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}
