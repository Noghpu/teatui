use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::Show,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

pub struct Tui {
    terminal: CrosstermTerminal,
    entered: bool,
}

impl Tui {
    pub fn new() -> Result<Self> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            entered: false,
        })
    }

    pub fn enter(&mut self) -> Result<()> {
        enable_raw_mode()?;
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err.into());
        }
        self.entered = true;

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = Self::reset_terminal();
            original_hook(panic_info);
        }));

        if let Err(err) = self.terminal.hide_cursor() {
            let _ = self.cleanup();
            return Err(err.into());
        }
        if let Err(err) = self.terminal.clear() {
            let _ = self.cleanup();
            return Err(err.into());
        }

        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        self.cleanup()
    }

    fn reset_terminal() -> Result<()> {
        let raw_result = disable_raw_mode();
        let screen_result = execute!(io::stdout(), Show, LeaveAlternateScreen);

        raw_result?;
        screen_result?;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<()> {
        if !self.entered {
            return Ok(());
        }

        let reset_result = Self::reset_terminal();
        let _ = self.terminal.show_cursor();
        self.entered = false;
        reset_result
    }

    pub fn draw<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal.draw(f)?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
