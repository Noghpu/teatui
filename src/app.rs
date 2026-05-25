use std::{collections::BTreeMap, path::PathBuf};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::{Action, Direction};
use crate::command::CommandRunner;
use crate::config::Config;
use crate::event::{AppEvent, EventHandler, JobResult, JobStatus};
use crate::generate::{Focus, GeneratePhase, GenerateState, InputMode};
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
#[allow(dead_code)]
pub struct JobRecord {
    pub id: u64,
    pub name: String,
    pub command: String,
    pub status: JobStatus,
    pub duration: Option<std::time::Duration>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Default, Clone)]
pub struct JobRegistry {
    pub jobs: BTreeMap<u64, JobRecord>,
    pub last_result: Option<JobResult>,
}

impl JobRegistry {
    pub fn record(&mut self, result: JobResult) {
        let entry = self.jobs.entry(result.id).or_insert_with(|| JobRecord {
            id: result.id,
            name: result.name.clone(),
            command: result.command.clone(),
            status: result.status,
            duration: result.duration,
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            timed_out: result.timed_out,
        });

        entry.name = result.name.clone();
        entry.command = result.command.clone();
        entry.status = result.status;
        entry.duration = result.duration;
        entry.stdout = result.stdout.clone();
        entry.stderr = result.stderr.clone();
        entry.timed_out = result.timed_out;
        self.last_result = Some(result);
    }

    pub fn status(&self) -> JobStatus {
        if self
            .jobs
            .values()
            .any(|job| job.status == JobStatus::Running)
        {
            return JobStatus::Running;
        }

        if self
            .jobs
            .values()
            .any(|job| job.status == JobStatus::Queued)
        {
            return JobStatus::Queued;
        }

        self.last_result
            .as_ref()
            .map(|result| result.status)
            .unwrap_or(JobStatus::Idle)
    }
}

pub struct App {
    config: Config,
    command_runner: CommandRunner,
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
    pub fn new(config: Config, command_runner: CommandRunner) -> Self {
        Self {
            config,
            command_runner,
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

    fn handle_key(&self, key: KeyEvent) -> Action {
        if self.input_mode == InputMode::Editing {
            return self.handle_edit_key(key);
        }

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

    fn handle_edit_key(&self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::CancelEdit,
            KeyCode::Enter => Action::CommitEdit,
            KeyCode::Backspace => Action::Backspace,
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                Action::InsertChar(ch)
            }
            _ => Action::Tick,
        }
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Back => self.back(),
            Action::Navigate(direction) => self.navigate(direction),
            Action::Focus(direction) => self.move_focus(direction),
            Action::Select => self.select(),
            Action::Edit => self.begin_editing_form_field(),
            Action::InsertChar(ch) => self.generate.insert_into_selected_field(ch),
            Action::Backspace => self.generate.backspace_selected_field(),
            Action::CommitEdit => self.finish_editing(true),
            Action::CancelEdit => self.finish_editing(false),
            Action::Generate => self.generate_pr(),
            Action::JobResult(result) => self.record_job_result(result),
            Action::Error(msg) => {
                tracing::error!("Error: {}", msg);
            }
            Action::Tick | Action::Render => {}
        }
    }

    fn back(&mut self) {
        match self.screen {
            Screen::Landing => {}
            Screen::Generate | Screen::PullRequests | Screen::Issues => {
                self.screen = Screen::Landing;
                self.focus = Focus::Menu;
                self.input_mode = InputMode::Normal;
            }
        }
    }

    fn navigate(&mut self, direction: Direction) {
        match (self.screen, self.focus, direction) {
            (Screen::Generate, Focus::Form, Direction::Up) => self.generate.move_field_up(),
            (Screen::Generate, Focus::Form, Direction::Down) => self.generate.move_field_down(),
            (Screen::Generate, _, Direction::Up) => self.generate.move_revset_up(),
            (Screen::Generate, _, Direction::Down) => self.generate.move_revset_down(),
            (Screen::Landing, _, Direction::Up) => {
                self.landing.selected_entry = self.landing.selected_entry.saturating_sub(1);
            }
            (Screen::Landing, _, Direction::Down) => {
                self.landing.selected_entry = (self.landing.selected_entry + 1).min(2);
            }
            (Screen::PullRequests, _, Direction::Up) => {
                self.pull_requests.selected_item =
                    self.pull_requests.selected_item.saturating_sub(1);
            }
            (Screen::PullRequests, _, Direction::Down) => {
                self.pull_requests.selected_item = (self.pull_requests.selected_item + 1).min(2);
            }
            (Screen::Issues, _, Direction::Up) => {
                self.issues.selected_item = self.issues.selected_item.saturating_sub(1);
            }
            (Screen::Issues, _, Direction::Down) => {
                self.issues.selected_item = (self.issues.selected_item + 1).min(2);
            }
        }
    }

    fn move_focus(&mut self, direction: Direction) {
        self.focus = match (self.focus, direction) {
            (Focus::Menu, Direction::Up) => Focus::Menu,
            (Focus::Menu, Direction::Down) => Focus::Form,
            (Focus::Form, Direction::Up) => Focus::Menu,
            (Focus::Form, Direction::Down) => Focus::Preview,
            (Focus::Preview, Direction::Up) => Focus::Form,
            (Focus::Preview, Direction::Down) => Focus::Preview,
        };
    }

    fn select(&mut self) {
        match self.screen {
            Screen::Landing => self.open_selected_landing_entry(),
            Screen::Generate if self.focus == Focus::Menu => self.select_revset(),
            Screen::Generate if self.focus == Focus::Form => self.begin_editing_form_field(),
            _ => {}
        }
    }

    fn open_selected_landing_entry(&mut self) {
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

    fn select_revset(&mut self) {
        self.focus = Focus::Form;
        self.generate.phase = GeneratePhase::EditingForm;
        self.generate.selected_field = 0;
        self.input_mode = InputMode::Normal;
    }

    fn begin_editing_form_field(&mut self) {
        if self.screen == Screen::Generate && self.focus == Focus::Form {
            self.generate.begin_editing_selected_field();
            self.input_mode = InputMode::Editing;
        }
    }

    fn finish_editing(&mut self, commit: bool) {
        if self.screen == Screen::Generate && self.focus == Focus::Form {
            if commit {
                self.generate.commit_selected_field();
            } else {
                self.generate.cancel_selected_field();
            }
        }
        self.input_mode = InputMode::Normal;
    }

    fn generate_pr(&mut self) {
        if self.screen == Screen::Generate {
            self.generate.phase = GeneratePhase::Generating;
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let command = self.command_runner.jj_status_command(cwd);
            let job_id = self.command_runner.spawn(command);
            self.logs.entries.push(format!(
                "job #{job_id} collecting status for revset {}",
                self.generate.selected_revset().label(),
            ));
        }
    }

    fn record_job_result(&mut self, result: JobResult) {
        self.jobs.record(result.clone());
        self.logs.entries.push(format!(
            "job #{} {} finished with {:?}",
            result.id, result.name, result.status
        ));
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
