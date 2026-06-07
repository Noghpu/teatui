pub mod generate;
pub mod landing;
pub mod status;

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
    /// User requested PR execution from a ready draft.
    Execute,
    /// User asked to copy the completed PR URL to the clipboard.
    CopyUrl,
    /// User asked to open the completed PR URL in the browser.
    OpenUrl,
}

pub enum NewScreen {
    Landing,
    Generate,
}
