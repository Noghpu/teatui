use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::action::{Action, Direction};
use crate::config::Config;
use crate::event::{AppEvent, EventHandler, JobResult, JobStatus};
use crate::generate::{FORM_FIELDS, Focus, GeneratePhase, GenerateState, InputMode};
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

#[derive(Debug, Default, Clone)]
pub struct RepoState {
    pub workspace_root: Option<PathBuf>,
    pub inside_workspace: bool,
    pub gitea_remote: Option<String>,
    pub tea_authenticated: Option<bool>,
    pub ollama_reachable: Option<bool>,
    pub base_branch: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct LogState {
    pub entries: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct LandingState {
    pub selected_entry: usize,
}

#[derive(Debug, Default, Clone)]
pub struct ListState {
    pub selected_item: usize,
}

#[derive(Debug, Clone)]
pub struct JobRegistry {
    pub status: JobStatus,
    pub last_result: Option<JobResult>,
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self {
            status: JobStatus::Idle,
            last_result: None,
        }
    }
}

pub struct App {
    config: Config,
    screen: Screen,
    focus: Focus,
    input_mode: InputMode,
    repo: RepoState,
    landing: LandingState,
    generate: GenerateState,
    pull_requests: ListState,
    issues: ListState,
    logs: LogState,
    jobs: JobRegistry,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            screen: Screen::Landing,
            focus: Focus::Menu,
            input_mode: InputMode::Normal,
            repo: RepoState::default(),
            landing: LandingState::default(),
            generate: GenerateState::demo(),
            pull_requests: ListState::default(),
            issues: ListState::default(),
            logs: LogState::default(),
            jobs: JobRegistry::default(),
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
                AppEvent::Job(result) => Action::JobResult(result),
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
            Action::Back => {
                if self.input_mode == InputMode::Editing {
                    self.input_mode = InputMode::Normal;
                    return;
                }

                match self.screen {
                    Screen::Landing => {}
                    Screen::Generate | Screen::PullRequests | Screen::Issues => {
                        self.screen = Screen::Landing;
                        self.focus = Focus::Menu;
                        self.input_mode = InputMode::Normal;
                    }
                }
            }
            Action::Navigate(Direction::Up) => match self.screen {
                Screen::Generate if self.focus == Focus::Form => {
                    self.generate.selected_field = self.generate.selected_field.saturating_sub(1);
                }
                Screen::Landing => {
                    self.landing.selected_entry = self.landing.selected_entry.saturating_sub(1);
                }
                Screen::PullRequests => {
                    self.pull_requests.selected_item =
                        self.pull_requests.selected_item.saturating_sub(1);
                }
                Screen::Issues => {
                    self.issues.selected_item = self.issues.selected_item.saturating_sub(1);
                }
                _ => {
                    self.generate.selected_revset = self.generate.selected_revset.saturating_sub(1);
                }
            },
            Action::Navigate(Direction::Down) => match self.screen {
                Screen::Generate if self.focus == Focus::Form => {
                    if self.generate.selected_field < FORM_FIELDS.len().saturating_sub(1) {
                        self.generate.selected_field += 1;
                    }
                }
                Screen::Landing => {
                    if self.landing.selected_entry < 2 {
                        self.landing.selected_entry += 1;
                    }
                }
                Screen::PullRequests => {
                    if self.pull_requests.selected_item < 2 {
                        self.pull_requests.selected_item += 1;
                    }
                }
                Screen::Issues => {
                    if self.issues.selected_item < 2 {
                        self.issues.selected_item += 1;
                    }
                }
                _ => {
                    if self.generate.selected_revset < self.generate.revsets.len().saturating_sub(1)
                    {
                        self.generate.selected_revset += 1;
                    }
                }
            },
            Action::Focus(Direction::Up) => {
                self.focus = match self.focus {
                    Focus::Menu => Focus::Menu,
                    Focus::Form => Focus::Menu,
                    Focus::Preview => Focus::Form,
                };
            }
            Action::Focus(Direction::Down) => {
                self.focus = match self.focus {
                    Focus::Menu => Focus::Form,
                    Focus::Form => Focus::Preview,
                    Focus::Preview => Focus::Preview,
                };
            }
            Action::Select => match self.screen {
                Screen::Landing => {
                    self.screen = match self.landing.selected_entry {
                        0 => Screen::Generate,
                        1 => Screen::PullRequests,
                        _ => Screen::Issues,
                    };
                    self.focus = Focus::Menu;
                    self.input_mode = InputMode::Normal;
                    if self.screen == Screen::Generate {
                        self.generate.phase = GeneratePhase::SelectingRevset;
                    }
                }
                Screen::Generate if self.focus == Focus::Menu => {
                    self.focus = Focus::Form;
                    self.generate.focus = Focus::Form;
                    self.generate.phase = GeneratePhase::EditingForm;
                    self.generate.selected_field = 0;
                    self.input_mode = InputMode::Normal;
                }
                Screen::Generate if self.focus == Focus::Form => {
                    self.input_mode = InputMode::Editing;
                }
                _ => {}
            },
            Action::Edit => {
                if self.screen == Screen::Generate && self.focus == Focus::Form {
                    self.input_mode = InputMode::Editing;
                    self.generate.focus = Focus::Form;
                }
            }
            Action::Generate => {
                if self.screen == Screen::Generate {
                    self.generate.phase = GeneratePhase::Generating;
                    self.logs.entries.push(format!(
                        "generate prompt for revset {}",
                        self.generate.selected_revset().label()
                    ));
                }
            }
            Action::JobResult(result) => {
                self.jobs.status = result.status;
                self.jobs.last_result = Some(result.clone());
                self.logs.entries.push(format!(
                    "job {} finished with {:?}",
                    result.name, result.status
                ));
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

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub fn repo(&self) -> &RepoState {
        &self.repo
    }

    pub fn landing(&self) -> &LandingState {
        &self.landing
    }

    pub fn generate(&self) -> &GenerateState {
        &self.generate
    }

    pub fn pull_requests(&self) -> &ListState {
        &self.pull_requests
    }

    pub fn issues(&self) -> &ListState {
        &self.issues
    }

    pub fn logs(&self) -> &LogState {
        &self.logs
    }

    pub fn jobs(&self) -> &JobRegistry {
        &self.jobs
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &Config {
        &self.config
    }
}
