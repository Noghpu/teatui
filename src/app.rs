use std::collections::BTreeMap;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::{Action, Direction};
use crate::command::CommandRunner;
use crate::config::Config;
use crate::context::{ContextCollector, ContextResult};
use crate::event::{AppEvent, EventHandler, GenerationResult, JobResult, JobStatus};
use crate::generate::{Focus, GeneratePhase, GenerateState, InputMode, RevsetSummary};
use crate::jj::RevsetDiscovery;
use crate::ollama::OllamaClient;
use crate::repo::{RepoDiscovery, RepoState};
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
    #[allow(dead_code)]
    command_runner: CommandRunner,
    generation_tx: UnboundedSender<Box<GenerationResult>>,
    context_tx: UnboundedSender<Box<ContextResult>>,
    repo_discovery: RepoDiscovery,
    revset_discovery: RevsetDiscovery,
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
    pub fn new(
        config: Config,
        command_runner: CommandRunner,
        generation_tx: UnboundedSender<Box<GenerationResult>>,
        context_tx: UnboundedSender<Box<ContextResult>>,
        repo_discovery: RepoDiscovery,
        revset_discovery: RevsetDiscovery,
    ) -> Self {
        let repo = RepoState::bootstrap(&config);
        Self {
            config,
            command_runner,
            generation_tx,
            context_tx,
            repo_discovery,
            revset_discovery,
            screen: Screen::Landing,
            focus: Focus::Menu,
            input_mode: InputMode::Normal,
            repo,
            landing: LandingState::default(),
            generate: GenerateState::with_placeholder("Revsets pending discovery"),
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
                AppEvent::Generation(result) => Action::GenerationResult(result),
                AppEvent::Context(context) => Action::Context(context),
                AppEvent::Repo(repo) => Action::RepoUpdated(repo),
                AppEvent::Revsets(revsets) => Action::RevsetsUpdated(revsets),
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
            KeyCode::Char('p') => Action::TogglePromptView,
            KeyCode::Char('r') => Action::Refresh,
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
            Action::TogglePromptView => self.toggle_prompt_view(),
            Action::Refresh => self.refresh(),
            Action::GenerationResult(result) => self.record_generation_result(*result),
            Action::Context(context) => self.apply_context(*context),
            Action::RepoUpdated(repo) => self.apply_repo(*repo),
            Action::RevsetsUpdated(revsets) => self.apply_revsets(revsets.revsets),
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
                self.update_input_mode();
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
        self.update_input_mode();
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
            0 if self.repo.inside_workspace => Screen::Generate,
            0 => {
                self.logs
                    .entries
                    .push("Generate PR requires a jj workspace".into());
                Screen::Landing
            }
            1 => Screen::PullRequests,
            _ => Screen::Issues,
        };
        self.focus = Focus::Menu;
        self.update_input_mode();
        if self.screen == Screen::Generate {
            self.generate.phase = GeneratePhase::SelectingRevset;
        }
    }

    fn select_revset(&mut self) {
        self.focus = Focus::Form;
        self.generate.phase = GeneratePhase::EditingForm;
        self.generate.selected_field = 0;
        self.update_input_mode();
        self.generate.sync_head_from_selected_revset();
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
        self.update_input_mode();
    }

    fn generate_pr(&mut self) {
        if self.screen == Screen::Generate {
            self.generate.validate_form();
            let blockers = self.generate.blocking_errors();
            if !blockers.is_empty() {
                let message = blockers.join("; ");
                self.logs
                    .entries
                    .push(format!("context collection blocked: {message}"));
                self.generate.fail_context_collection(message);
                self.focus = Focus::Form;
                self.update_input_mode();
                return;
            }

            match self.generate.phase {
                GeneratePhase::ContextReady | GeneratePhase::DraftReady | GeneratePhase::Failed
                    if self.generate.context.is_some() =>
                {
                    self.start_generation();
                }
                GeneratePhase::Generating => {
                    self.logs
                        .entries
                        .push("generation already in progress".into());
                }
                _ => self.start_context_collection(),
            }
        }
    }

    fn start_context_collection(&mut self) {
        self.generate.begin_context_collection();
        let selected_revset = self.generate.selected_revset().clone();
        let form = self.generate.form.clone();
        let collector = ContextCollector::new(
            &self.config,
            self.repo.clone(),
            form,
            selected_revset.clone(),
        );
        let tx = self.context_tx.clone();
        self.logs.entries.push(format!(
            "collecting context for revset {}",
            selected_revset.label(),
        ));
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || collector.collect())
                .await
                .unwrap_or_else(|err| {
                    Err(crate::context::ContextError {
                        command: "context collection".into(),
                        message: err.to_string(),
                        stdout: String::new(),
                        stderr: String::new(),
                    })
                });

            let context = match result {
                Ok(bundle) => ContextResult::ready(bundle),
                Err(error) => ContextResult::failed(error),
            };
            let _ = tx.send(Box::new(context));
        });
    }

    fn start_generation(&mut self) {
        let Some(prompt) = self.generate.prompt_build() else {
            self.logs
                .entries
                .push("generation blocked: prompt context is unavailable".into());
            self.generate
                .fail_generation("prompt context is unavailable");
            return;
        };

        let prompt_bytes = prompt.manifest.byte_count;
        let selected_revset = self.generate.selected_revset().label().to_string();
        let client = match OllamaClient::new(&self.config) {
            Ok(client) => client,
            Err(err) => {
                self.generate.fail_generation(err.to_string());
                self.logs
                    .entries
                    .push(format!("generation failed: {}", err));
                return;
            }
        };

        self.generate.begin_generation();
        self.logs.entries.push(format!(
            "sending prompt to ollama for {} ({} bytes)",
            selected_revset, prompt_bytes
        ));
        self.logs.entries.push("ollama request in progress".into());

        let tx = self.generation_tx.clone();
        tokio::spawn(async move {
            let result = client.generate_draft(&prompt).await;
            let event = match result {
                Ok(draft) => GenerationResult::Ready(draft),
                Err(error) => GenerationResult::Failed(error),
            };
            let _ = tx.send(Box::new(event));
        });
    }

    fn toggle_prompt_view(&mut self) {
        if self.screen == Screen::Generate {
            self.generate.toggle_prompt_view();
        }
    }

    pub fn refresh(&self) {
        self.repo_discovery.refresh();
        self.revset_discovery.refresh();
    }

    fn apply_repo(&mut self, repo: RepoState) {
        let inside_workspace = repo.inside_workspace;
        self.repo = repo;

        if self.screen == Screen::Generate && !inside_workspace {
            self.logs
                .entries
                .push("Generate PR blocked: cwd is not inside a jj workspace".into());
            self.screen = Screen::Landing;
            self.focus = Focus::Menu;
            self.update_input_mode();
        }
    }

    fn apply_revsets(&mut self, revsets: Vec<RevsetSummary>) {
        self.generate.replace_revsets(revsets);
        if self.screen == Screen::Generate {
            self.logs
                .entries
                .push(format!("loaded {} jj revsets", self.generate.revsets.len()));
        }
    }

    fn record_job_result(&mut self, result: JobResult) {
        self.jobs.record(result.clone());
        self.logs.entries.push(format!(
            "job #{} {} finished with {:?}",
            result.id, result.name, result.status
        ));
    }

    fn record_generation_result(&mut self, result: GenerationResult) {
        match result {
            GenerationResult::Ready(draft) => {
                self.log_raw_model_response(&draft.raw_model_response);
                self.generate.complete_generation(draft.clone());
                self.logs.entries.push(format!(
                    "ollama generation finished for {}",
                    draft.branch_name
                ));
            }
            GenerationResult::Failed(error) => {
                if let Some(raw_response) = error.raw_response.as_ref() {
                    self.log_raw_model_response(raw_response);
                }
                self.generate.fail_generation(error.message.clone());
                self.logs
                    .entries
                    .push(format!("ollama generation failed: {}", error.message));
            }
        }
    }

    fn log_raw_model_response(&mut self, raw_response: &str) {
        self.logs.entries.push("ollama raw response:".into());
        if raw_response.trim().is_empty() {
            self.logs.entries.push("(empty)".into());
            return;
        }

        for line in raw_response.lines() {
            self.logs.entries.push(line.to_string());
        }
    }

    fn apply_context(&mut self, context: ContextResult) {
        match context {
            ContextResult::Ready(bundle) => {
                if bundle.repo_identity.selected_revset != self.generate.selected_revset().label() {
                    let stale = format!(
                        "discarded stale context for {}; selected revset is {}",
                        bundle.repo_identity.selected_revset,
                        self.generate.selected_revset().label()
                    );
                    self.logs.entries.push(stale.clone());
                    self.generate.fail_context_collection(stale);
                    return;
                }

                self.generate.complete_context_collection(*bundle);
                self.log_context_bundle();
                self.logs.entries.push(format!(
                    "context ready for {}",
                    self.generate.selected_revset().label()
                ));
            }
            ContextResult::Failed(error) => {
                self.generate.fail_context_collection(error.display());
                self.logs
                    .entries
                    .push(format!("context collection failed: {}", error.display()));
                self.log_command_capture(
                    "context failed",
                    &error.command,
                    &error.stdout,
                    &error.stderr,
                );
            }
        }
    }

    fn log_context_bundle(&mut self) {
        if let Some(context) = self.generate.context.as_ref() {
            let repo_root = context
                .repo_identity
                .workspace_root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "(unknown)".into());
            let selected_revset = context.selected_revset.label().to_string();
            let status = context.status.clone();
            let revset_log = context.revset_log.clone();
            let diff_stats = context.diff_stats.clone();
            let diff = context.diff.clone();

            self.logs
                .entries
                .push(format!("context repo root: {repo_root}"));
            self.logs
                .entries
                .push(format!("context selected revset: {selected_revset}"));
            self.log_command_capture("jj status", &status.command, &status.stdout, &status.stderr);
            self.log_command_capture(
                "jj log",
                &revset_log.command,
                &revset_log.stdout,
                &revset_log.stderr,
            );
            self.log_command_capture(
                "jj diff --stat",
                &diff_stats.command,
                &diff_stats.stdout,
                &diff_stats.stderr,
            );
            self.log_command_capture("jj diff", &diff.command, &diff.stdout, &diff.stderr);
        }
    }

    fn log_command_capture(&mut self, label: &str, command: &str, stdout: &str, stderr: &str) {
        self.logs.entries.push(format!("{label}: {command}"));
        if !stdout.trim().is_empty() {
            self.logs
                .entries
                .push(format!("{label} stdout: {}", stdout.trim()));
        }
        if !stderr.trim().is_empty() {
            self.logs
                .entries
                .push(format!("{label} stderr: {}", stderr.trim()));
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

    fn update_input_mode(&mut self) {
        self.input_mode = match (self.screen, self.focus) {
            (Screen::Generate, Focus::Preview) => InputMode::Review,
            _ => InputMode::Normal,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc::unbounded_channel;

    fn test_app() -> App {
        let config = Config::default();
        let (job_tx, _job_rx) = unbounded_channel();
        let (generation_tx, _generation_rx) = unbounded_channel();
        let (context_tx, _context_rx) = unbounded_channel();
        let (repo_tx, _repo_rx) = unbounded_channel();
        let (revset_tx, _revset_rx) = unbounded_channel();
        App::new(
            config.clone(),
            CommandRunner::new(&config, job_tx),
            generation_tx,
            context_tx,
            RepoDiscovery::new(config.clone(), repo_tx),
            RevsetDiscovery::new(&config, ".", revset_tx),
        )
    }

    #[test]
    fn normal_mode_routes_navigation_and_actions() {
        let app = test_app();

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty())),
            Action::Navigate(Direction::Down)
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            Action::Select
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty())),
            Action::Quit
        );
    }

    #[test]
    fn input_mode_routes_printable_keys_into_the_field() {
        let mut app = test_app();
        app.input_mode = InputMode::Editing;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty())),
            Action::InsertChar('g')
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty())),
            Action::InsertChar('j')
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty())),
            Action::InsertChar('q')
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            Action::CancelEdit
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            Action::CommitEdit
        );
    }

    #[test]
    fn generate_action_stays_in_form_when_form_fields_are_invalid() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.focus = Focus::Form;
        app.generate.form.branch_name = crate::generate::FieldState::new("Feature Bad");

        app.generate_pr();

        assert_eq!(app.generate.phase, GeneratePhase::Failed);
        assert_eq!(app.focus, Focus::Form);
        assert!(app.logs.entries[0].contains("context collection blocked"));
    }
}
