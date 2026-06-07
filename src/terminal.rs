use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::Result;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal as RatatuiTerminal, backend::CrosstermBackend};

pub type Backend = RatatuiTerminal<CrosstermBackend<Stdout>>;

static KITTY_ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct Terminal {
    inner: Backend,
    teardown_done: bool,
}

impl Terminal {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, Hide)?;

        let kitty = execute!(
            out,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )
        .is_ok();
        KITTY_ACTIVE.store(kitty, Ordering::Relaxed);
        tracing::debug!(target: "teatui::terminal", kitty, "enhancement flags");

        install_panic_hook();

        let backend = CrosstermBackend::new(io::stdout());
        let mut inner = RatatuiTerminal::new(backend)?;
        inner.clear()?;

        Ok(Self {
            inner,
            teardown_done: false,
        })
    }

    pub fn frame(&mut self) -> &mut Backend {
        &mut self.inner
    }

    fn teardown() -> io::Result<()> {
        let mut out = io::stdout();
        if KITTY_ACTIVE.swap(false, Ordering::Relaxed) {
            let _ = execute!(out, PopKeyboardEnhancementFlags);
        }
        execute!(out, Show, LeaveAlternateScreen)?;
        disable_raw_mode()?;
        out.flush()?;
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.teardown_done {
            return;
        }
        self.teardown_done = true;
        if let Err(e) = Self::teardown() {
            tracing::error!(target: "teatui::terminal", error = %e, "teardown failed");
        }
    }
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = Terminal::teardown();
        tracing::error!(target: "teatui::panic", payload = %info, "panic captured");
        original(info);
    }));
}
