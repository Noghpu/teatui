use color_eyre::eyre::Result;

use crate::action::{Action, Direction};
use crate::config::Config;
use crate::event::{AppEvent, EventHandler};
use crate::tui::Tui;
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Landing,
    Generate,
    Issues,
    PullRequests,
    Logs,
}

impl View {
    pub const ALL: [Self; 5] = [
        Self::Landing,
        Self::Generate,
        Self::Issues,
        Self::PullRequests,
        Self::Logs,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Landing => "Landing",
            Self::Generate => "Generate PR",
            Self::Issues => "Issues",
            Self::PullRequests => "PRs",
            Self::Logs => "Logs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Navigation,
    Work,
    Preview,
}

pub struct App {
    config: Config,
    selected_view: usize,
    focused_pane: Pane,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            selected_view: 0,
            focused_pane: Pane::Navigation,
            should_quit: false,
        }
    }

    pub async fn run(&mut self, tui: &mut Tui, mut events: EventHandler) -> Result<()> {
        loop {
            tui.draw(|frame| ui::render(frame, self))?;

            let action = match events.next().await? {
                AppEvent::Tick => Action::Tick,
                AppEvent::Key(key) => self.handle_key(key),
                AppEvent::Resize(_, _) => Action::Render,
            };

            self.update(action);

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_key(&self, key: crossterm::event::KeyEvent) -> Action {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => Action::Navigate(Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => Action::Navigate(Direction::Down),
            KeyCode::Left | KeyCode::Char('h') => Action::Focus(Direction::Up),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => Action::Focus(Direction::Down),
            KeyCode::Enter => Action::Select,
            _ => Action::Tick,
        }
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Navigate(Direction::Up) => {
                self.selected_view = self.selected_view.saturating_sub(1);
            }
            Action::Navigate(Direction::Down) => {
                if self.selected_view < View::ALL.len().saturating_sub(1) {
                    self.selected_view += 1;
                }
            }
            Action::Focus(Direction::Up) => {
                self.focused_pane = match self.focused_pane {
                    Pane::Navigation => Pane::Navigation,
                    Pane::Work => Pane::Navigation,
                    Pane::Preview => Pane::Work,
                };
            }
            Action::Focus(Direction::Down) => {
                self.focused_pane = match self.focused_pane {
                    Pane::Navigation => Pane::Work,
                    Pane::Work => Pane::Preview,
                    Pane::Preview => Pane::Preview,
                };
            }
            Action::Select => {
                tracing::info!("Selected view: {}", self.current_view().title());
            }
            Action::Error(msg) => {
                tracing::error!("Error: {}", msg);
            }
            Action::Tick | Action::Render => {}
        }
    }

    pub fn views(&self) -> &'static [View] {
        &View::ALL
    }

    pub fn selected_view_index(&self) -> usize {
        self.selected_view
    }

    pub fn current_view(&self) -> View {
        View::ALL[self.selected_view]
    }

    pub fn focused_pane(&self) -> Pane {
        self.focused_pane
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &Config {
        &self.config
    }
}
