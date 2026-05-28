use std::collections::VecDeque;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::{Action, Direction};
use crate::command::run_plan_sequentially;
use crate::context::{self, ContextResult};
use crate::event::{
    AppEvent, BackgroundEvent, EventHandler, ExecutionOutcome, GenerationResult, JobResult,
    JobStatus,
};
use crate::generate::{
    ExecutionPlan, Focus, GeneratePhase, GenerateState, InputMode, RevsetSummary, StaleCheckResult,
    validate_for_execution,
};
use crate::jj;
use crate::llm::LlmClient;
use crate::repo::{self, RepoState};
use crate::tui::Tui;
use crate::ui;

const LOG_CAP: usize = 500;

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
    pub entries: VecDeque<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl From<JobResult> for JobRecord {
    fn from(result: JobResult) -> Self {
        Self {
            id: result.id,
            name: result.name,
            command: result.command,
            status: result.status,
            duration: result.duration,
            stdout: result.stdout,
            stderr: result.stderr,
            timed_out: result.timed_out,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct JobRegistry {
    pub records: Vec<JobRecord>,
}

impl JobRegistry {
    pub fn upsert(&mut self, result: JobResult) {
        let record = JobRecord::from(result);
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            *existing = record;
        } else {
            self.records.push(record);
        }
    }

    pub fn active_status(&self) -> Option<JobStatus> {
        self.records
            .iter()
            .rev()
            .find(|record| record.status.is_active())
            .map(|record| record.status)
    }
}

#[derive(Debug, Default, Clone)]
pub struct LandingState {
    pub selected_entry: usize,
}

#[derive(Debug, Default, Clone)]
pub struct ListState {
    pub selected_item: usize,
}

pub struct App {
    config: crate::config::Config,
    bg_tx: UnboundedSender<BackgroundEvent>,
    cwd: PathBuf,
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
    pub fn new(config: crate::config::Config, bg_tx: UnboundedSender<BackgroundEvent>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let repo = RepoState::new(&config);
        Self {
            config,
            bg_tx,
            cwd,
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

            match events.next().await? {
                AppEvent::Tick => {}
                AppEvent::Resize => self.handle_resize(),
                AppEvent::Key(key) => self.update(self.handle_key(key)),
                AppEvent::Background(event) => self.handle_background(event),
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_resize(&mut self) {
        for field in self.generate.form.editors_mut() {
            field.reset_editor_viewport();
        }
    }

    fn handle_key(&self, key: KeyEvent) -> Action {
        if self.input_mode == InputMode::Editing {
            return self.handle_edit_key(key);
        }

        if self.screen == Screen::Generate {
            match self.generate.phase {
                GeneratePhase::CheckingFreshness => {
                    return match key.code {
                        KeyCode::Esc => Action::Back,
                        _ => Action::Tick,
                    };
                }
                GeneratePhase::Confirming => {
                    return match key.code {
                        KeyCode::Esc => Action::Back,
                        KeyCode::Enter => Action::ExecuteConfirmed,
                        _ => Action::Tick,
                    };
                }
                GeneratePhase::Executing => {
                    return match key.code {
                        KeyCode::Esc => Action::Tick,
                        _ => Action::Tick,
                    };
                }
                GeneratePhase::DraftReady => {
                    if matches!(key.code, KeyCode::Char('c')) {
                        return Action::ConfirmExecution;
                    }
                }
                GeneratePhase::Failed => {
                    if matches!(key.code, KeyCode::Char('c')) {
                        return Action::ConfirmExecution;
                    }
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc => Action::Back,
            KeyCode::Up | KeyCode::Char('k') => Action::Navigate(Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => Action::Navigate(Direction::Down),
            KeyCode::Left | KeyCode::Char('h') => Action::FocusPrev,
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => Action::FocusNext,
            KeyCode::Char('i') => Action::Edit,
            KeyCode::Char('g') => Action::Generate,
            KeyCode::Char('c') => Action::Tick,
            KeyCode::Char('p') => Action::TogglePromptView,
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Enter => Action::Select,
            _ => Action::Tick,
        }
    }

    fn handle_edit_key(&self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc
            | KeyCode::Enter
            | KeyCode::Backspace
            | KeyCode::Char(_)
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => Action::EditKey(key),
            _ => Action::Tick,
        }
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Tick => {}
            Action::Quit => self.should_quit = true,
            Action::Back => self.back(),
            Action::Navigate(direction) => self.navigate(direction),
            Action::FocusNext => self.move_focus(true),
            Action::FocusPrev => self.move_focus(false),
            Action::Select => self.select(),
            Action::Edit => self.begin_editing_form_field(),
            Action::EditKey(key) => self.apply_edit_key(key),
            Action::Generate => self.generate_pr(),
            Action::ConfirmExecution => self.confirm_execution(),
            Action::ExecuteConfirmed => self.execute_confirmed(),
            Action::TogglePromptView => self.toggle_prompt_view(),
            Action::Refresh => self.refresh(),
        }
    }

    fn handle_background(&mut self, event: BackgroundEvent) {
        match event {
            BackgroundEvent::Generation(result) => self.apply_generation(result),
            BackgroundEvent::Context(result) => self.apply_context(result),
            BackgroundEvent::Repo(repo) => self.apply_repo(*repo),
            BackgroundEvent::Revsets(revsets) => self.apply_revsets(revsets),
            BackgroundEvent::StaleCheck(result) => self.apply_stale_check(result),
            BackgroundEvent::Job(job) => self.apply_job(job),
            BackgroundEvent::ExecutionStep { index, total } => {
                self.apply_execution_step(index, total)
            }
            BackgroundEvent::ExecutionDone(outcome) => self.apply_execution_done(outcome),
        }
    }

    fn back(&mut self) {
        if self.screen == Screen::Generate
            && matches!(
                self.generate.phase,
                GeneratePhase::CheckingFreshness | GeneratePhase::Confirming
            )
        {
            self.generate.cancel_confirmation();
            self.focus = Focus::Form;
            self.input_mode = InputMode::Normal;
            self.log("execution preview cancelled");
            return;
        }

        if self.screen == Screen::Generate && self.generate.phase == GeneratePhase::Executing {
            self.log("execution in progress");
            return;
        }

        if self.screen == Screen::Generate && self.generate.phase == GeneratePhase::Complete {
            self.generate.clear_completion_state();
            self.generate.phase = GeneratePhase::DraftReady;
            self.focus = Focus::Form;
            self.input_mode = InputMode::Normal;
            self.log("execution results cleared");
            return;
        }

        if self.screen != Screen::Landing {
            self.screen = Screen::Landing;
            self.focus = Focus::Menu;
            self.input_mode = InputMode::Normal;
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

    fn move_focus(&mut self, forward: bool) {
        self.focus = match (self.focus, forward) {
            (Focus::Menu, true) => Focus::Form,
            (Focus::Menu, false) => Focus::Menu,
            (Focus::Form, true) => Focus::Preview,
            (Focus::Form, false) => Focus::Menu,
            (Focus::Preview, true) => Focus::Preview,
            (Focus::Preview, false) => Focus::Form,
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
            0 if self.repo.inside_workspace || self.repo.discovering => Screen::Generate,
            0 => {
                self.log("Generate PR requires a jj workspace");
                Screen::Landing
            }
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
        self.generate.sync_head_from_selected_revset();
    }

    fn begin_editing_form_field(&mut self) {
        if self.screen == Screen::Generate && self.focus == Focus::Form {
            self.generate.begin_editing_selected_field();
            self.input_mode = InputMode::Editing;
        }
    }

    fn apply_edit_key(&mut self, key: KeyEvent) {
        if self.screen != Screen::Generate || self.focus != Focus::Form {
            return;
        }

        match key.code {
            KeyCode::Esc => self.finish_editing(false),
            KeyCode::Enter => {
                if self.generate.selected_field() == crate::generate::FieldId::Description {
                    self.generate
                        .form
                        .field_mut(self.generate.selected_field())
                        .input(key);
                } else {
                    self.finish_editing(true);
                }
            }
            KeyCode::Char('s')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.generate.selected_field() == crate::generate::FieldId::Description =>
            {
                self.finish_editing(true);
            }
            _ => self.generate.input_selected_field(key),
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
        if self.screen != Screen::Generate {
            return;
        }

        self.generate.validate_form();
        let blockers = self.generate.blocking_errors();
        if !blockers.is_empty() {
            let message = blockers.join("; ");
            self.log(format!("context collection blocked: {message}"));
            self.generate.fail_context_collection(message);
            self.focus = Focus::Form;
            return;
        }

        match self.generate.phase {
            GeneratePhase::ContextReady | GeneratePhase::DraftReady | GeneratePhase::Failed
                if self.generate.context.is_some() =>
            {
                self.start_generation();
            }
            GeneratePhase::Generating => {
                self.log("generation already in progress");
            }
            _ => self.start_context_collection(),
        }
    }

    fn confirm_execution(&mut self) {
        if self.screen != Screen::Generate
            || !matches!(
                self.generate.phase,
                GeneratePhase::DraftReady | GeneratePhase::Failed
            )
        {
            return;
        }

        self.generate.validate_form();
        match validate_for_execution(&self.generate.form, &self.repo) {
            Ok(()) => {}
            Err(errors) => {
                for error in errors {
                    self.log(format!("execution validation failed: {error}"));
                }
                self.focus = Focus::Form;
                self.input_mode = InputMode::Normal;
                return;
            }
        }

        let Some(expected_commit_ids) = self
            .generate
            .context
            .as_ref()
            .map(|context| context.selected_revset.commit_ids().to_vec())
        else {
            self.log("execution confirmation blocked: prompt context is unavailable");
            self.generate
                .fail_confirmation("prompt context is unavailable");
            self.focus = Focus::Form;
            self.input_mode = InputMode::Normal;
            return;
        };

        self.generate.begin_confirmation_check();
        self.focus = Focus::Preview;
        self.input_mode = InputMode::Normal;
        self.log("execution validation passed");
        self.log("verifying repo context before showing execution preview");

        let cwd = self
            .repo
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.cwd.clone());
        let selected_revset = self.generate.selected_revset().label().to_string();
        jj::spawn_stale_context_check(
            &self.config,
            cwd,
            selected_revset,
            expected_commit_ids,
            self.bg_tx.clone(),
        );
    }

    fn execute_confirmed(&mut self) {
        if self.screen != Screen::Generate || self.generate.phase != GeneratePhase::Confirming {
            return;
        }

        let Some(plan) = self.generate.execution_plan.clone() else {
            self.log("execution blocked: missing execution plan");
            self.generate.fail_execution(None, "missing execution plan");
            self.focus = Focus::Form;
            self.input_mode = InputMode::Normal;
            return;
        };

        self.generate.begin_execution();
        self.focus = Focus::Preview;
        self.input_mode = InputMode::Normal;
        self.log("execution started");

        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let outcome = run_plan_sequentially(plan, tx.clone()).await;
            let _ = tx.send(BackgroundEvent::ExecutionDone(outcome));
        });
    }

    fn start_context_collection(&mut self) {
        self.generate.begin_context_collection();
        let selected_revset = self.generate.selected_revset().clone();
        let form = self.generate.form.clone();
        let config = self.config.clone();
        let repo = self.repo.clone();
        let tx = self.bg_tx.clone();
        self.log(format!(
            "collecting context for revset {}",
            selected_revset.label(),
        ));
        tokio::spawn(async move {
            let result = context::collect(&config, repo, form, selected_revset).await;
            let context = match result {
                Ok(bundle) => ContextResult::ready(bundle),
                Err(error) => ContextResult::failed(error),
            };
            let _ = tx.send(BackgroundEvent::Context(context));
        });
    }

    fn start_generation(&mut self) {
        let Some(prompt) = self.generate.prompt().cloned() else {
            self.log("generation blocked: prompt context is unavailable");
            self.generate
                .fail_generation("prompt context is unavailable");
            return;
        };

        let prompt_bytes = prompt.manifest.byte_count;
        let selected_revset = self.generate.selected_revset().label().to_string();
        let Some(backend) = self.config.llm.active_backend().cloned() else {
            self.generate
                .fail_generation("active LLM backend is unavailable");
            self.log("generation failed: active LLM backend is unavailable");
            return;
        };

        let client = match LlmClient::from_config(&backend) {
            Ok(client) => client,
            Err(err) => {
                let message = err.to_string();
                self.generate.fail_generation(&message);
                self.log(format!("generation failed: {message}"));
                return;
            }
        };

        self.generate.begin_generation();
        self.log(format!(
            "sending prompt to llm backend {} for {selected_revset} ({prompt_bytes} bytes)",
            backend.name
        ));
        self.log("llm request in progress");

        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = client.generate_draft(&prompt).await;
            let event = match result {
                Ok(draft) => GenerationResult::Ready(draft),
                Err(error) => GenerationResult::Failed(error),
            };
            let _ = tx.send(BackgroundEvent::Generation(event));
        });
    }

    fn toggle_prompt_view(&mut self) {
        if self.screen == Screen::Generate {
            self.generate.toggle_prompt_view();
        }
    }

    pub fn refresh(&self) {
        repo::spawn_discovery(self.config.clone(), self.cwd.clone(), self.bg_tx.clone());
        jj::spawn_revset_discovery(&self.config, self.cwd.clone(), self.bg_tx.clone());
    }

    fn apply_repo(&mut self, repo: RepoState) {
        let inside_workspace = repo.inside_workspace;
        self.repo = repo;

        if self.screen == Screen::Generate && !inside_workspace {
            self.log("Generate PR blocked: cwd is not inside a jj workspace");
            self.screen = Screen::Landing;
            self.focus = Focus::Menu;
            self.input_mode = InputMode::Normal;
        }
    }

    fn apply_revsets(&mut self, revsets: Vec<RevsetSummary>) {
        self.generate.replace_revsets(revsets);
        if self.screen == Screen::Generate {
            let count = self.generate.revsets.len();
            self.log(format!("loaded {count} jj revsets"));
        }
    }

    fn apply_generation(&mut self, result: GenerationResult) {
        match result {
            GenerationResult::Ready(draft) => {
                self.log_raw_model_response(&draft.raw_model_response);
                let branch = draft.branch_name.clone();
                self.generate.complete_generation(draft);
                self.log(format!("llm generation finished for {branch}"));
            }
            GenerationResult::Failed(error) => {
                if let Some(raw_response) = error.raw_response.as_ref() {
                    self.log_raw_model_response(raw_response);
                }
                let message = error.message.clone();
                self.generate.fail_generation(&message);
                self.log(format!("llm generation failed: {message}"));
            }
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
                    self.log(stale.clone());
                    self.generate.fail_context_collection(stale);
                    return;
                }

                self.generate.complete_context_collection(*bundle);
                self.log_context_bundle();
                self.log(format!(
                    "context ready for {}",
                    self.generate.selected_revset().label()
                ));
            }
            ContextResult::Failed(error) => {
                let display = error.display();
                self.generate.fail_context_collection(display.clone());
                self.log(format!("context collection failed: {display}"));
                self.log_command_capture(
                    "context failed",
                    &error.command,
                    &error.stdout,
                    &error.stderr,
                );
            }
        }
    }

    fn apply_stale_check(&mut self, result: StaleCheckResult) {
        if self.screen != Screen::Generate
            || self.generate.phase != GeneratePhase::CheckingFreshness
        {
            return;
        }

        match result {
            StaleCheckResult::Fresh => {
                let plan = ExecutionPlan::from_draft(
                    &self.generate.form,
                    &self.repo,
                    self.generate.selected_revset(),
                    &self.config,
                );
                self.generate.complete_confirmation(plan);
                self.focus = Focus::Preview;
                self.input_mode = InputMode::Confirm;
                self.log("repo context verified for execution preview");
            }
            StaleCheckResult::Stale { reason } => {
                self.generate.fail_confirmation(reason.clone());
                self.focus = Focus::Form;
                self.input_mode = InputMode::Normal;
                self.log(format!("execution confirmation failed: {reason}"));
                self.log("press r to refresh revsets/context");
            }
        }
    }

    fn apply_job(&mut self, job: JobResult) {
        let status = job.status;
        self.jobs.upsert(job.clone());

        if matches!(status, JobStatus::Failed | JobStatus::TimedOut) {
            let label = format!("job {}", job.name);
            self.log_command_capture(&label, &job.command, &job.stdout, &job.stderr);
        }
    }

    fn apply_execution_step(&mut self, index: usize, total: usize) {
        self.generate.record_execution_step(index, total);
        self.log(format!("execution step {}/{}", index + 1, total));
    }

    fn apply_execution_done(&mut self, outcome: ExecutionOutcome) {
        if let Some(message) = outcome.message.as_ref() {
            self.log(message.clone());
        }

        if let Some(failed_step) = outcome.failed_step {
            let message = outcome
                .message
                .clone()
                .unwrap_or_else(|| "execution step failed".into());
            self.generate
                .fail_execution(Some(failed_step), message.clone());
            self.log(format!(
                "execution failed at step {}: {}",
                failed_step + 1,
                message
            ));
            return;
        }

        let plan = self.generate.execution_plan.clone().unwrap_or_default();
        self.generate
            .complete_execution(outcome.pr_url.clone(), plan);
        if let Some(pr_url) = outcome.pr_url {
            self.log(format!("PR created: {pr_url}"));
        } else {
            self.log("execution completed without a parsed PR URL");
        }
        self.log("execution complete");
    }

    fn log_raw_model_response(&mut self, raw_response: &str) {
        self.log("llm raw response:");
        if raw_response.trim().is_empty() {
            self.log("(empty)");
            return;
        }

        for line in raw_response.lines() {
            self.log(line.to_string());
        }
    }

    fn log_context_bundle(&mut self) {
        let Some(context) = self.generate.context.as_ref() else {
            return;
        };
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

        self.log(format!("context repo root: {repo_root}"));
        self.log(format!("context selected revset: {selected_revset}"));
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

    fn log_command_capture(&mut self, label: &str, command: &str, stdout: &str, stderr: &str) {
        self.log(format!("{label}: {command}"));
        if !stdout.trim().is_empty() {
            self.log(format!("{label} stdout: {}", stdout.trim()));
        }
        if !stderr.trim().is_empty() {
            self.log(format!("{label} stderr: {}", stderr.trim()));
        }
    }

    pub fn log(&mut self, message: impl Into<String>) {
        if self.logs.entries.len() >= LOG_CAP {
            self.logs.entries.pop_front();
        }
        self.logs.entries.push_back(message.into());
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::generate::FieldId;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc::unbounded_channel;

    fn test_app() -> App {
        let config = Config::default();
        let (tx, _rx) = unbounded_channel();
        App::new(config, tx)
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
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::empty())),
            Action::FocusNext
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::empty())),
            Action::FocusPrev
        );
    }

    #[test]
    fn input_mode_routes_printable_keys_into_the_field() {
        let mut app = test_app();
        app.input_mode = InputMode::Editing;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::empty())),
            Action::EditKey(KeyEvent::new(KeyCode::Home, KeyModifiers::empty()))
        );
    }

    #[test]
    fn single_line_enter_commits_without_inserting_newline() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.focus = Focus::Form;
        app.generate.selected_field = FieldId::ALL
            .iter()
            .position(|field| *field == FieldId::Title)
            .expect("title field");

        app.update(Action::Edit);
        app.update(Action::EditKey(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::empty(),
        )));
        app.update(Action::EditKey(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::empty(),
        )));

        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.generate.form.title.value, "x");
        assert!(!app.generate.form.title.value.contains('\n'));
    }

    #[test]
    fn description_enter_inserts_newline_and_ctrl_s_commits() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.focus = Focus::Form;
        app.generate.selected_field = FieldId::ALL
            .iter()
            .position(|field| *field == FieldId::Description)
            .expect("description field");

        app.update(Action::Edit);
        app.update(Action::EditKey(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::empty(),
        )));
        app.update(Action::EditKey(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::empty(),
        )));
        app.update(Action::EditKey(KeyEvent::new(
            KeyCode::Char('y'),
            KeyModifiers::empty(),
        )));

        assert_eq!(app.input_mode, InputMode::Editing);
        assert_eq!(app.generate.form.description.display_value(), "x\ny");

        app.update(Action::EditKey(KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
        )));

        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.generate.form.description.value, "x\ny");
    }

    #[test]
    fn draft_ready_maps_c_to_confirm_execution() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::DraftReady;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty())),
            Action::ConfirmExecution
        );
    }

    #[test]
    fn confirm_mode_routes_enter_and_escape() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::Confirming;
        app.input_mode = InputMode::Confirm;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            Action::ExecuteConfirmed
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            Action::Back
        );
    }

    #[test]
    fn back_from_confirming_returns_to_draft_ready_and_clears_preview() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::Confirming;
        app.generate.execution_plan = Some(ExecutionPlan::default());
        app.generate.confirmation_summary = Some("validation passed".into());
        app.generate.freshness_result = Some(crate::generate::StaleCheckResult::Fresh);
        app.input_mode = InputMode::Confirm;

        app.back();

        assert_eq!(app.generate.phase, GeneratePhase::DraftReady);
        assert!(app.generate.execution_plan.is_none());
        assert!(app.generate.confirmation_summary.is_none());
        assert!(app.generate.freshness_result.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.focus, Focus::Form);
    }

    #[test]
    fn back_from_complete_returns_to_draft_ready_and_clears_completion() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::Complete;
        app.generate.completion = Some(crate::generate::Completion {
            pr_url: Some("https://code.example.com/team/project/pulls/1".into()),
            plan: ExecutionPlan::default(),
        });
        app.input_mode = InputMode::Normal;

        app.back();

        assert_eq!(app.generate.phase, GeneratePhase::DraftReady);
        assert!(app.generate.completion.is_none());
        assert_eq!(app.focus, Focus::Form);
    }

    #[test]
    fn failed_phase_maps_c_to_confirm_execution() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::Failed;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty())),
            Action::ConfirmExecution
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

    #[test]
    fn log_buffer_caps_at_log_cap() {
        let mut app = test_app();
        for i in 0..LOG_CAP + 50 {
            app.log(format!("entry {i}"));
        }
        assert_eq!(app.logs.entries.len(), LOG_CAP);
        assert_eq!(app.logs.entries.front().unwrap(), "entry 50");
    }
}
