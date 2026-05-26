use std::collections::VecDeque;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::{Action, Direction};
use crate::context::{self, ContextResult};
use crate::event::{AppEvent, BackgroundEvent, EventHandler, GenerationResult};
use crate::generate::{Focus, GeneratePhase, GenerateState, InputMode, RevsetSummary};
use crate::ollama::OllamaClient;
use crate::repo::{self, RepoState};
use crate::tui::Tui;
use crate::{jj, ui};

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
    should_quit: bool,
}

impl App {
    pub fn new(config: crate::config::Config, bg_tx: UnboundedSender<BackgroundEvent>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let repo = RepoState::bootstrap(&config);
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
            should_quit: false,
        }
    }

    pub async fn run(&mut self, tui: &mut Tui, mut events: EventHandler) -> Result<()> {
        loop {
            tui.draw(|frame| ui::render(frame, self))?;

            match events.next().await? {
                AppEvent::Tick | AppEvent::Resize => {}
                AppEvent::Key(key) => self.update(self.handle_key(key)),
                AppEvent::Background(event) => self.handle_background(event),
            }

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
            KeyCode::Left | KeyCode::Char('h') => Action::FocusPrev,
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => Action::FocusNext,
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
            Action::Tick => {}
            Action::Quit => self.should_quit = true,
            Action::Back => self.back(),
            Action::Navigate(direction) => self.navigate(direction),
            Action::FocusNext => self.move_focus(true),
            Action::FocusPrev => self.move_focus(false),
            Action::Select => self.select(),
            Action::Edit => self.begin_editing_form_field(),
            Action::InsertChar(ch) => self.generate.insert_into_selected_field(ch),
            Action::Backspace => self.generate.backspace_selected_field(),
            Action::CommitEdit => self.finish_editing(true),
            Action::CancelEdit => self.finish_editing(false),
            Action::Generate => self.generate_pr(),
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
        }
    }

    fn back(&mut self) {
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
            0 if self.repo.inside_workspace => Screen::Generate,
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
        let client = match OllamaClient::new(&self.config) {
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
            "sending prompt to ollama for {selected_revset} ({prompt_bytes} bytes)"
        ));
        self.log("ollama request in progress");

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
                self.log(format!("ollama generation finished for {branch}"));
            }
            GenerationResult::Failed(error) => {
                if let Some(raw_response) = error.raw_response.as_ref() {
                    self.log_raw_model_response(raw_response);
                }
                let message = error.message.clone();
                self.generate.fail_generation(&message);
                self.log(format!("ollama generation failed: {message}"));
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

    fn log_raw_model_response(&mut self, raw_response: &str) {
        self.log("ollama raw response:");
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
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
            Action::InsertChar('g')
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
