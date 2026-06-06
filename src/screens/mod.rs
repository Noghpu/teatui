pub mod backend_picker;
pub mod generate;
pub mod landing;
pub mod theme;
mod util;
mod widgets;

use crate::domain::JjOp;

pub use generate::GenerateState;
pub use landing::LandingState;

pub enum Screen {
    Landing(LandingState),
    /// Boxed because `GenerateState` carries the (potentially large)
    /// `ContextBundle` + `PromptBuild` once generation is in flight, and
    /// we don't want every `Screen` to pay that footprint.
    Generate(Box<GenerateState>),
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Landing(LandingState::default())
    }
}

/// Outcome of dispatching an input event to a screen handler. The runtime
/// applies it to the App (set dirty flag, quit, navigate, …) without the
/// screen needing `&mut App`.
pub enum Transition {
    None,
    Dirty,
    Quit,
    Navigate(NewScreen),
    /// User requested PR generation for the currently-selected head.
    Generate,
    /// User asked to abort an in-flight generation (context collection or LLM).
    CancelGeneration,
    /// User asked to review the concrete shell commands for a ready draft.
    ReviewExecution,
    /// User requested PR execution from a ready draft.
    Execute,
    /// User asked to copy the completed PR URL to the clipboard.
    CopyUrl,
    /// User asked to open the completed PR URL in the browser.
    OpenUrl,
    /// User asked to refresh the revset list.
    RefreshRevsets,
    /// User asked to open the LLM backend switcher.
    OpenBackendPicker,
    /// User confirmed a jj stack-shaping operation from the Changes pane.
    JjOp(JjOp),
    /// User pressed `G` to start stacked-PR generation from the selected heads.
    GenerateStack,
    /// User asked to cancel an in-flight stack generation or close the review modal.
    CancelStack,
    /// User asked to push the highlighted PR in the bulk review modal.
    PushStackPr(usize),
    /// User asked to push the whole stack oldest-to-newest in the bulk review modal.
    PushStackAll,
}

pub enum NewScreen {
    Landing,
    Generate,
}
