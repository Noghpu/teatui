use color_eyre::eyre::Result;

use crate::action::{Action, Direction};
use crate::config::Config;
use crate::event::{AppEvent, EventHandler};
use crate::tui::Tui;
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Landing,
    Generate,
    PullRequests,
    Issues,
}

impl Screen {
    pub fn title(self) -> &'static str {
        match self {
            Self::Landing => "Landing",
            Self::Generate => "Generate PR",
            Self::PullRequests => "Manage PRs",
            Self::Issues => "Manage Issues",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Menu,
    Form,
    Preview,
}

#[derive(Debug, Clone)]
pub struct RevsetSummary {
    label: String,
    description: String,
    bookmarks: Vec<String>,
    stats: String,
}

impl RevsetSummary {
    fn new(label: &str, description: &str, bookmarks: &[&str], stats: &str) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            bookmarks: bookmarks
                .iter()
                .map(|bookmark| (*bookmark).into())
                .collect(),
            stats: stats.into(),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn bookmarks(&self) -> &[String] {
        &self.bookmarks
    }

    pub fn stats(&self) -> &str {
        &self.stats
    }
}

const FORM_FIELDS: [&str; 8] = [
    "head",
    "branch name",
    "base",
    "title",
    "description",
    "labels",
    "assignees",
    "milestone",
];

const LANDING_ENTRIES: [Screen; 3] = [Screen::Generate, Screen::PullRequests, Screen::Issues];

const SECONDARY_ITEMS: [&str; 3] = ["Open items", "Filter", "Comment"];

pub struct App {
    config: Config,
    screen: Screen,
    focused_pane: Pane,
    selected_landing_entry: usize,
    revsets: Vec<RevsetSummary>,
    selected_revset: usize,
    selected_field: usize,
    selected_secondary_item: usize,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            screen: Screen::Landing,
            focused_pane: Pane::Menu,
            selected_landing_entry: 0,
            revsets: vec![
                RevsetSummary::new(
                    "@",
                    "Current working copy change",
                    &["teatui-ui"],
                    "3 files changed, +142 -12",
                ),
                RevsetSummary::new(
                    "heads(trunk()..)",
                    "Current stack above trunk",
                    &[],
                    "8 files changed, +426 -38",
                ),
                RevsetSummary::new("@-", "Parent change", &["main@origin"], "clean baseline"),
            ],
            selected_revset: 0,
            selected_field: 0,
            selected_secondary_item: 0,
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
            KeyCode::Esc => Action::Back,
            KeyCode::Up | KeyCode::Char('k') => Action::Navigate(Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => Action::Navigate(Direction::Down),
            KeyCode::Left | KeyCode::Char('h') => Action::Focus(Direction::Up),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => Action::Focus(Direction::Down),
            KeyCode::Char('i') => Action::Edit,
            KeyCode::Char('g') => Action::Generate,
            KeyCode::Enter => Action::Select,
            _ => Action::Tick,
        }
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Back => match self.screen {
                Screen::Landing => {}
                Screen::Generate | Screen::PullRequests | Screen::Issues => {
                    self.screen = Screen::Landing;
                    self.focused_pane = Pane::Menu;
                }
            },
            Action::Navigate(Direction::Up) => {
                if self.screen == Screen::Generate && self.focused_pane == Pane::Form {
                    self.selected_field = self.selected_field.saturating_sub(1);
                } else if self.screen == Screen::Landing {
                    self.selected_landing_entry = self.selected_landing_entry.saturating_sub(1);
                } else if self.screen == Screen::PullRequests || self.screen == Screen::Issues {
                    self.selected_secondary_item = self.selected_secondary_item.saturating_sub(1);
                } else {
                    self.selected_revset = self.selected_revset.saturating_sub(1);
                }
            }
            Action::Navigate(Direction::Down) => {
                if self.screen == Screen::Generate && self.focused_pane == Pane::Form {
                    if self.selected_field < FORM_FIELDS.len().saturating_sub(1) {
                        self.selected_field += 1;
                    }
                } else if self.screen == Screen::Landing {
                    if self.selected_landing_entry < LANDING_ENTRIES.len().saturating_sub(1) {
                        self.selected_landing_entry += 1;
                    }
                } else if self.screen == Screen::PullRequests || self.screen == Screen::Issues {
                    if self.selected_secondary_item < SECONDARY_ITEMS.len().saturating_sub(1) {
                        self.selected_secondary_item += 1;
                    }
                } else if self.selected_revset < self.revsets.len().saturating_sub(1) {
                    self.selected_revset += 1;
                }
            }
            Action::Focus(Direction::Up) => {
                self.focused_pane = match self.focused_pane {
                    Pane::Menu => Pane::Menu,
                    Pane::Form => Pane::Menu,
                    Pane::Preview => Pane::Form,
                };
            }
            Action::Focus(Direction::Down) => {
                self.focused_pane = match self.focused_pane {
                    Pane::Menu => Pane::Form,
                    Pane::Form => Pane::Preview,
                    Pane::Preview => Pane::Preview,
                };
            }
            Action::Select => {
                if self.screen == Screen::Landing {
                    self.screen = LANDING_ENTRIES[self.selected_landing_entry];
                    self.focused_pane = Pane::Menu;
                } else if self.screen == Screen::Generate && self.focused_pane == Pane::Menu {
                    self.focused_pane = Pane::Form;
                    self.selected_field = 0;
                } else if self.focused_pane == Pane::Form {
                    tracing::info!("Edit field: {}", self.selected_field_name());
                }
            }
            Action::Edit => {
                if self.screen == Screen::Generate && self.focused_pane == Pane::Form {
                    tracing::info!("Edit field: {}", self.selected_field_name());
                }
            }
            Action::Generate => {
                if self.screen == Screen::Generate {
                    tracing::info!(
                        "Generate PR prompt for revset: {}",
                        self.selected_revset().label()
                    );
                }
            }
            Action::Error(msg) => {
                tracing::error!("Error: {}", msg);
            }
            Action::Tick | Action::Render => {}
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn landing_entries(&self) -> &'static [Screen] {
        &LANDING_ENTRIES
    }

    pub fn selected_landing_entry_index(&self) -> usize {
        self.selected_landing_entry
    }

    pub fn revsets(&self) -> &[RevsetSummary] {
        &self.revsets
    }

    pub fn selected_revset_index(&self) -> usize {
        self.selected_revset
    }

    pub fn selected_revset(&self) -> &RevsetSummary {
        &self.revsets[self.selected_revset]
    }

    pub fn form_fields(&self) -> &'static [&'static str] {
        &FORM_FIELDS
    }

    pub fn selected_field_index(&self) -> usize {
        self.selected_field
    }

    pub fn selected_field_name(&self) -> &'static str {
        FORM_FIELDS[self.selected_field]
    }

    pub fn focused_pane(&self) -> Pane {
        self.focused_pane
    }

    pub fn secondary_items(&self) -> &'static [&'static str] {
        &SECONDARY_ITEMS
    }

    pub fn selected_secondary_item_index(&self) -> usize {
        self.selected_secondary_item
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &Config {
        &self.config
    }
}
