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
    JobStatus, PrCommentResult, PullRequestsResult,
};
use crate::external;
use crate::generate::{
    ExecutionPlan, Focus, GeneratePhase, GenerateState, InputMode, PrForm, RevsetSummary,
    StaleCheckResult, TextFieldState, validate_for_execution,
};
use crate::jj;
use crate::llm::LlmClient;
use crate::pull_requests::PullRequestSummary;
use crate::repo::{self, RepoState};
use crate::repo_options::{self, RepoOptions, RepoOptionsResult};
use crate::tea;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrCommentPhase {
    #[default]
    Idle,
    Editing,
    Submitting,
    Failed,
}

#[derive(Debug, Default, Clone)]
pub struct LandingState {
    pub selected_entry: usize,
}

#[derive(Debug, Default, Clone)]
pub struct ListState {
    pub selected_item: usize,
    pub preview_scroll: crate::generate::ScrollState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PullRequestLoadStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed,
}

impl PullRequestLoadStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PullRequestState {
    pub items: Vec<PullRequestSummary>,
    pub selected_item: usize,
    pub filter: TextFieldState,
    pub load_status: PullRequestLoadStatus,
    pub load_error: Option<String>,
    pub preview_scroll: crate::generate::ScrollState,
    pub next_request_id: u64,
    pub active_request_id: Option<u64>,
    pub comment_phase: PrCommentPhase,
    pub comment_buffer: String,
    pub comment_cursor: usize,
    pub comment_error: Option<String>,
}

impl Default for PullRequestState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected_item: 0,
            filter: TextFieldState::new(""),
            load_status: PullRequestLoadStatus::Idle,
            load_error: None,
            preview_scroll: crate::generate::ScrollState::default(),
            next_request_id: 1,
            active_request_id: None,
            comment_phase: PrCommentPhase::Idle,
            comment_buffer: String::new(),
            comment_cursor: 0,
            comment_error: None,
        }
    }
}

impl PullRequestState {
    pub fn begin_load(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.active_request_id = Some(request_id);
        self.load_status = PullRequestLoadStatus::Loading;
        self.load_error = None;
        request_id
    }

    pub fn is_loading(&self) -> bool {
        self.active_request_id.is_some()
    }

    pub fn load_status_label(&self) -> &'static str {
        self.load_status.label()
    }

    pub fn begin_filter_edit(&mut self) {
        self.filter.begin_edit();
    }

    pub fn input_filter(&mut self, key: crossterm::event::KeyEvent) {
        self.filter.input(key);
        self.clamp_selection();
    }

    pub fn commit_filter(&mut self) {
        self.filter.commit();
        self.clamp_selection();
    }

    pub fn cancel_filter(&mut self) {
        self.filter.cancel();
        self.clamp_selection();
    }

    pub fn reset_filter_editor_viewport(&mut self) {
        self.filter.reset_editor_viewport();
    }

    pub fn selected_visible_index(&self) -> usize {
        self.selected_item
    }

    pub fn visible_items(&self) -> Vec<(usize, &PullRequestSummary)> {
        let filter = self.filter.display_value().trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| pull_request_matches_filter(item, &filter))
            .collect()
    }

    pub fn selected_item(&self) -> Option<&PullRequestSummary> {
        self.visible_items()
            .get(self.selected_item)
            .map(|(_, item)| *item)
    }

    pub fn visible_count(&self) -> usize {
        self.visible_items().len()
    }

    pub fn move_selected_up(&mut self) {
        if self.visible_count() > 0 {
            self.selected_item = self.selected_item.saturating_sub(1);
        }
    }

    pub fn move_selected_down(&mut self) {
        let visible = self.visible_count();
        if visible > 0 {
            self.selected_item = (self.selected_item + 1).min(visible.saturating_sub(1));
        }
    }

    pub fn set_items(&mut self, items: Vec<PullRequestSummary>) {
        self.items = items;
        self.load_status = PullRequestLoadStatus::Ready;
        self.load_error = None;
        self.clamp_selection();
    }

    pub fn fail_load(&mut self, message: String) {
        self.load_status = PullRequestLoadStatus::Failed;
        self.load_error = Some(message);
        self.active_request_id = None;
        self.clamp_selection();
    }

    pub fn complete_load(&mut self, request_id: u64, items: Vec<PullRequestSummary>) -> bool {
        if self.active_request_id != Some(request_id) {
            return false;
        }

        self.active_request_id = None;
        self.set_items(items);
        true
    }

    pub fn fail_request(&mut self, request_id: u64, message: String) -> bool {
        if self.active_request_id != Some(request_id) {
            return false;
        }

        self.fail_load(message);
        true
    }

    pub fn open_comment_modal(&mut self) {
        self.comment_phase = PrCommentPhase::Editing;
        self.comment_error = None;
    }

    pub fn close_comment_modal(&mut self) {
        self.comment_phase = PrCommentPhase::Idle;
        self.comment_buffer.clear();
        self.comment_cursor = 0;
        self.comment_error = None;
    }

    pub fn comment_input_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char(ch) => {
                self.comment_buffer.insert(self.comment_cursor, ch);
                self.comment_cursor += ch.len_utf8();
            }
            KeyCode::Backspace if self.comment_cursor > 0 => {
                let prev = self
                    .comment_buffer
                    .char_indices()
                    .rev()
                    .find(|(i, _)| *i < self.comment_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.comment_buffer.drain(prev..self.comment_cursor);
                self.comment_cursor = prev;
            }
            KeyCode::Delete if self.comment_cursor < self.comment_buffer.len() => {
                let next = self.comment_buffer[self.comment_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| self.comment_cursor + i)
                    .unwrap_or(self.comment_buffer.len());
                self.comment_buffer.drain(self.comment_cursor..next);
            }
            KeyCode::Left => {
                self.comment_cursor = self
                    .comment_buffer
                    .char_indices()
                    .rev()
                    .find(|(i, _)| *i < self.comment_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
            KeyCode::Right => {
                self.comment_cursor = self.comment_buffer[self.comment_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| self.comment_cursor + i)
                    .unwrap_or(self.comment_buffer.len());
            }
            KeyCode::Home => {
                self.comment_cursor = 0;
            }
            KeyCode::End => {
                self.comment_cursor = self.comment_buffer.len();
            }
            _ => {}
        }
    }

    fn clamp_selection(&mut self) {
        let visible = self.visible_count();
        if visible == 0 {
            self.selected_item = 0;
        } else {
            self.selected_item = self.selected_item.min(visible - 1);
        }
    }
}

fn pull_request_matches_filter(item: &PullRequestSummary, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }

    let mut haystack = String::new();
    haystack.push_str(&item.index.to_string());
    haystack.push(' ');
    haystack.push_str(&item.title);
    haystack.push(' ');
    haystack.push_str(&item.state);
    haystack.push(' ');
    haystack.push_str(&item.author);
    haystack.push(' ');
    haystack.push_str(&item.head);
    haystack.push(' ');
    haystack.push_str(&item.base);
    haystack.push(' ');
    haystack.push_str(&item.updated);
    haystack.push(' ');
    haystack.push_str(&item.url);
    haystack.push(' ');
    haystack.push_str(&item.body);
    for label in &item.labels {
        haystack.push(' ');
        haystack.push_str(label);
    }

    haystack.to_lowercase().contains(filter)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GenerationRequest {
    id: u64,
    selected_revset: String,
    form: PrForm,
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
    pull_requests: PullRequestState,
    issues: ListState,
    logs: LogState,
    jobs: JobRegistry,
    next_generation_id: u64,
    active_generation: Option<GenerationRequest>,
    repo_options: RepoOptions,
    should_quit: bool,
    pub status_message: Option<String>,
}

/// Pane-local key dispatch for the Generate screen in Normal input mode.
///
/// Returns `Some(action)` when the key is consumed by a pane-local rule.
/// Returns `None` for keys that should fall through to the global match
/// (arrows, j/k, Tab, Shift+Tab, Esc, q, Ctrl+C, Enter, etc.).
fn dispatch_generate_normal(focus: Focus, key: KeyEvent) -> Option<Action> {
    match (focus, key.code) {
        // Menu pane: r refreshes the revset list.
        (Focus::Menu, KeyCode::Char('r')) => Some(Action::Refresh),
        // Menu pane: g / p / i / c are no-ops when Menu is focused.
        (
            Focus::Menu,
            KeyCode::Char('g') | KeyCode::Char('p') | KeyCode::Char('i') | KeyCode::Char('c'),
        ) => Some(Action::Tick),

        // Form pane: g runs generate, i begins editing.
        (Focus::Form, KeyCode::Char('g')) => Some(Action::Generate),
        (Focus::Form, KeyCode::Char('i')) => Some(Action::Edit),
        // Form pane: p / c / r are no-ops when Form is focused.
        (Focus::Form, KeyCode::Char('p') | KeyCode::Char('c') | KeyCode::Char('r')) => {
            Some(Action::Tick)
        }

        // Preview pane: p toggles the prompt view, g regenerates.
        (Focus::Preview, KeyCode::Char('p')) => Some(Action::TogglePromptView),
        (Focus::Preview, KeyCode::Char('g')) => Some(Action::Generate),
        // Preview pane: i / r are no-ops when Preview is focused.
        (Focus::Preview, KeyCode::Char('i') | KeyCode::Char('r')) => Some(Action::Tick),

        // All other keys (global navigation, Enter, Esc, q, Tab, …) fall through.
        _ => None,
    }
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
            pull_requests: PullRequestState::default(),
            issues: ListState::default(),
            logs: LogState::default(),
            jobs: JobRegistry::default(),
            next_generation_id: 1,
            active_generation: None,
            repo_options: RepoOptions::default(),
            should_quit: false,
            status_message: None,
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
        self.pull_requests.reset_filter_editor_viewport();
    }

    fn handle_key(&self, key: KeyEvent) -> Action {
        // Comment modal input mode: capture all keys, including during submission.
        if self.screen == Screen::PullRequests
            && self.pull_requests.comment_phase != PrCommentPhase::Idle
        {
            return self.handle_comment_modal_key(key);
        }

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
                GeneratePhase::DraftReady
                    if matches!(key.code, KeyCode::Char('c')) && self.focus == Focus::Preview =>
                {
                    return Action::ConfirmExecution;
                }
                GeneratePhase::Failed
                    if matches!(key.code, KeyCode::Char('c')) && self.focus == Focus::Preview =>
                {
                    return Action::ConfirmExecution;
                }
                _ => {}
            }

            // Pane-local dispatch for Generate / Normal mode.
            // Keys that belong to a specific pane are silently ignored when
            // a different pane is focused.  Truly global keys (Tab, Shift+Tab,
            // Esc, q, Ctrl+C, arrows/j/k) fall through to the global match.
            if let Some(action) = dispatch_generate_normal(self.focus, key) {
                return action;
            }
        }

        // PR comment shortcut: 'c' opens comment modal when a PR is selected.
        if self.screen == Screen::PullRequests
            && matches!(key.code, KeyCode::Char('c'))
            && self.pull_requests.selected_item().is_some()
            && self.pull_requests.comment_phase == PrCommentPhase::Idle
        {
            return Action::OpenCommentModal;
        }

        // PR open-in-browser shortcut: 'o' opens the selected PR URL.
        if self.screen == Screen::PullRequests
            && self.input_mode == InputMode::Normal
            && self.pull_requests.comment_phase == PrCommentPhase::Idle
            && matches!(key.code, KeyCode::Char('o'))
            && self.pull_requests.selected_item().is_some()
        {
            return Action::OpenPrInBrowser;
        }

        // PR yank-URL shortcut: 'y' copies the selected PR URL.
        if self.screen == Screen::PullRequests
            && self.input_mode == InputMode::Normal
            && self.pull_requests.comment_phase == PrCommentPhase::Idle
            && matches!(key.code, KeyCode::Char('y'))
            && self.pull_requests.selected_item().is_some()
        {
            return Action::CopyPrUrl;
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

    fn handle_comment_modal_key(&self, key: KeyEvent) -> Action {
        // While the submission is in flight, swallow all keys so global
        // shortcuts (q/Esc/navigation) cannot leak out of the modal.
        if self.pull_requests.comment_phase == PrCommentPhase::Submitting {
            return Action::Tick;
        }
        match key.code {
            KeyCode::Esc => Action::CancelComment,
            KeyCode::Enter => Action::SubmitComment,
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Char(_) => Action::EditKey(key),
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
        // Clear any transient status message on each non-Tick user action so it
        // does not linger across unrelated interactions.  The OpenPrInBrowser
        // and CopyPrUrl handlers will set a fresh message after the clear.
        self.status_message = None;
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
            Action::OpenCommentModal => self.open_comment_modal(),
            Action::SubmitComment => self.submit_comment(),
            Action::CancelComment => self.cancel_comment(),
            Action::OpenPrInBrowser => self.open_selected_pr_in_browser(),
            Action::CopyPrUrl => self.copy_selected_pr_url(),
        }
    }

    fn handle_background(&mut self, event: BackgroundEvent) {
        match event {
            BackgroundEvent::Generation(result) => self.apply_generation(result),
            BackgroundEvent::Context(result) => self.apply_context(result),
            BackgroundEvent::Repo(repo) => self.apply_repo(*repo),
            BackgroundEvent::Revsets(revsets) => self.apply_revsets(revsets),
            BackgroundEvent::PullRequests(result) => self.apply_pull_requests(result),
            BackgroundEvent::PrComment(result) => self.apply_pr_comment(result),
            BackgroundEvent::StaleCheck(result) => self.apply_stale_check(result),
            BackgroundEvent::Job(job) => self.apply_job(job),
            BackgroundEvent::ExecutionStep { index, total } => {
                self.apply_execution_step(index, total)
            }
            BackgroundEvent::ExecutionDone(outcome) => self.apply_execution_done(outcome),
            BackgroundEvent::RepoOptions(result) => self.apply_repo_options(*result),
        }
    }

    fn back(&mut self) {
        // Cancel comment modal on Esc if editing.
        if self.screen == Screen::PullRequests
            && self.pull_requests.comment_phase != PrCommentPhase::Idle
            && self.pull_requests.comment_phase != PrCommentPhase::Submitting
        {
            self.pull_requests.close_comment_modal();
            return;
        }

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
            (Screen::Generate, Focus::Preview, Direction::Up) => {
                self.generate.scroll_preview_up();
            }
            (Screen::Generate, Focus::Preview, Direction::Down) => {
                self.generate.scroll_preview_down();
            }
            (Screen::Generate, Focus::Menu, Direction::Up) => self.generate.move_revset_up(),
            (Screen::Generate, Focus::Menu, Direction::Down) => self.generate.move_revset_down(),
            (Screen::Landing, _, Direction::Up) => {
                self.landing.selected_entry = self.landing.selected_entry.saturating_sub(1);
            }
            (Screen::Landing, _, Direction::Down) => {
                self.landing.selected_entry = (self.landing.selected_entry + 1).min(2);
            }
            (Screen::PullRequests, Focus::Menu, Direction::Up) => {
                self.pull_requests.move_selected_up();
            }
            (Screen::PullRequests, Focus::Menu, Direction::Down) => {
                self.pull_requests.move_selected_down();
            }
            (Screen::PullRequests, Focus::Form, _) => {}
            (Screen::PullRequests, Focus::Preview, Direction::Up) => {
                self.pull_requests.preview_scroll.scroll_up();
            }
            (Screen::PullRequests, Focus::Preview, Direction::Down) => {
                self.pull_requests.preview_scroll.scroll_down();
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
            Screen::PullRequests if self.focus == Focus::Form => {
                self.begin_editing_pull_request_filter()
            }
            _ => {}
        }
    }

    fn open_selected_landing_entry(&mut self) {
        self.screen = match self.landing.selected_entry {
            0 if self.repo.inside_workspace && !self.repo.discovering => Screen::Generate,
            0 if self.repo.discovering => Screen::Landing,
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
            // Trigger repo options load (stale-while-revalidate) on Generate PR entry.
            self.spawn_repo_options_load(false);
        } else if self.screen == Screen::PullRequests {
            self.spawn_pull_requests_load(false);
        }
    }

    fn select_revset(&mut self) {
        self.focus = Focus::Form;
        self.generate.phase = GeneratePhase::EditingForm;
        self.generate.selected_field = 0;
        self.generate.sync_head_from_selected_revset();
    }

    fn begin_editing_form_field(&mut self) {
        match self.screen {
            Screen::Generate if self.focus == Focus::Form => {
                self.generate.begin_editing_selected_field();
                self.input_mode = InputMode::Editing;
            }
            Screen::PullRequests if self.focus == Focus::Form => {
                self.begin_editing_pull_request_filter();
            }
            _ => {}
        }
    }

    fn begin_editing_pull_request_filter(&mut self) {
        if self.screen == Screen::PullRequests && self.focus == Focus::Form {
            self.pull_requests.begin_filter_edit();
            self.input_mode = InputMode::Editing;
        }
    }

    fn apply_edit_key(&mut self, key: KeyEvent) {
        // Comment modal has its own edit path; guard here to be safe.
        if self.screen == Screen::PullRequests
            && matches!(
                self.pull_requests.comment_phase,
                PrCommentPhase::Editing | PrCommentPhase::Failed
            )
        {
            self.pull_requests.comment_input_key(key);
            return;
        }

        match (self.screen, self.focus) {
            (Screen::Generate, Focus::Form) => match key.code {
                KeyCode::Esc => self.finish_editing(false),
                KeyCode::Enter => {
                    if self.generate.selected_field().kind().is_multiline() {
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
                        && self.generate.selected_field().kind().is_multiline() =>
                {
                    self.finish_editing(true);
                }
                _ => self.generate.input_selected_field(key),
            },
            (Screen::PullRequests, Focus::Form) => match key.code {
                KeyCode::Esc => self.finish_editing_pull_request_filter(false),
                KeyCode::Enter => self.finish_editing_pull_request_filter(true),
                _ => self.pull_requests.input_filter(key),
            },
            _ => {}
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

    fn finish_editing_pull_request_filter(&mut self, commit: bool) {
        if self.screen == Screen::PullRequests && self.focus == Focus::Form {
            if commit {
                self.pull_requests.commit_filter();
            } else {
                self.pull_requests.cancel_filter();
            }
        }
        self.input_mode = InputMode::Normal;
    }

    fn open_comment_modal(&mut self) {
        if self.screen != Screen::PullRequests {
            return;
        }
        if self.pull_requests.selected_item().is_none() {
            return;
        }
        self.pull_requests.open_comment_modal();
    }

    fn cancel_comment(&mut self) {
        self.pull_requests.close_comment_modal();
    }

    fn open_selected_pr_in_browser(&mut self) {
        if self.screen != Screen::PullRequests {
            return;
        }
        let Some(pr) = self.pull_requests.selected_item() else {
            return;
        };
        let url = pr.url.trim().to_string();
        if url.is_empty() {
            let msg = "error: PR has no URL".to_string();
            self.log(msg.clone());
            self.status_message = Some(msg);
            return;
        }
        match external::open_in_browser(&url) {
            Ok(()) => {
                let msg = format!("opened {url} in browser");
                self.log(msg.clone());
                self.status_message = Some(msg);
            }
            Err(e) => {
                let msg = format!("error: {e}");
                self.log(msg.clone());
                self.status_message = Some(msg);
            }
        }
    }

    fn copy_selected_pr_url(&mut self) {
        if self.screen != Screen::PullRequests {
            return;
        }
        let Some(pr) = self.pull_requests.selected_item() else {
            return;
        };
        let url = pr.url.trim().to_string();
        if url.is_empty() {
            let msg = "error: PR has no URL".to_string();
            self.log(msg.clone());
            self.status_message = Some(msg);
            return;
        }
        match external::copy_to_clipboard(&url) {
            Ok(()) => {
                let msg = format!("copied {url} to clipboard");
                self.log(msg.clone());
                self.status_message = Some(msg);
            }
            Err(e) => {
                let msg = format!("error: {e}");
                self.log(msg.clone());
                self.status_message = Some(msg);
            }
        }
    }

    fn submit_comment(&mut self) {
        if self.screen != Screen::PullRequests
            || !matches!(
                self.pull_requests.comment_phase,
                PrCommentPhase::Editing | PrCommentPhase::Failed
            )
        {
            return;
        }

        let body = self.pull_requests.comment_buffer.trim().to_string();
        if body.is_empty() {
            self.pull_requests.comment_error = Some("Comment cannot be empty.".into());
            return;
        }

        let Some(pr) = self.pull_requests.selected_item().cloned() else {
            self.pull_requests.comment_error = Some("No PR selected.".into());
            return;
        };

        self.pull_requests.comment_phase = PrCommentPhase::Submitting;
        self.pull_requests.comment_error = None;
        let pr_index = pr.index;
        self.log(format!("submitting comment on PR #{pr_index}"));

        tea::spawn_pr_comment(
            self.config.clone(),
            self.cwd.clone(),
            pr_index,
            body,
            self.bg_tx.clone(),
        );
    }

    fn apply_pr_comment(&mut self, result: PrCommentResult) {
        match result {
            PrCommentResult::Succeeded {
                pr_index,
                command,
                stdout,
                stderr,
            } => {
                self.log(format!("comment on PR #{pr_index} succeeded"));
                self.log_command_capture(
                    &format!("tea comment #{pr_index}"),
                    &command,
                    &stdout,
                    &stderr,
                );
                self.pull_requests.close_comment_modal();
            }
            PrCommentResult::Failed {
                pr_index,
                command,
                message,
                stdout,
                stderr,
            } => {
                self.log(format!("comment on PR #{pr_index} failed: {message}"));
                self.log_command_capture(
                    &format!("tea comment #{pr_index}"),
                    &command,
                    &stdout,
                    &stderr,
                );
                self.pull_requests.comment_phase = PrCommentPhase::Failed;
                self.pull_requests.comment_error = Some(message);
            }
        }
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
        let selected_revset = self
            .generate
            .context
            .as_ref()
            .map(|context| context.repo_identity.selected_revset.clone())
            .unwrap_or_else(|| self.generate.form.head.display_value().trim().to_string());
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
        let head = self.generate.form.head.display_value().trim().to_string();
        let form = self.generate.form.clone();
        let config = self.config.clone();
        let repo = self.repo.clone();
        let tx = self.bg_tx.clone();
        self.log(format!("collecting context for revset {head}"));
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
        let selected_revset = prompt.manifest.selected_revset.clone();
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
        let request_id = self.next_generation_id;
        self.next_generation_id += 1;
        self.active_generation = Some(GenerationRequest {
            id: request_id,
            selected_revset: selected_revset.clone(),
            form: self.generate.form.clone(),
        });
        self.log(format!(
            "sending prompt to llm backend {} for {selected_revset} ({prompt_bytes} bytes)",
            backend.name
        ));
        self.log("llm request in progress");

        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = client.generate_draft(&prompt).await;
            let event = match result {
                Ok(draft) => GenerationResult::Ready { request_id, draft },
                Err(error) => GenerationResult::Failed { request_id, error },
            };
            let _ = tx.send(BackgroundEvent::Generation(event));
        });
    }

    fn toggle_prompt_view(&mut self) {
        if self.screen == Screen::Generate {
            self.generate.toggle_prompt_view();
        }
    }

    pub fn refresh(&mut self) {
        repo::spawn_discovery(self.config.clone(), self.cwd.clone(), self.bg_tx.clone());
        jj::spawn_revset_discovery(&self.config, self.cwd.clone(), self.bg_tx.clone());
        self.spawn_repo_options_load(true);
        if self.screen == Screen::PullRequests {
            self.spawn_pull_requests_load(true);
        }
    }

    fn spawn_repo_options_load(&self, force_refresh: bool) {
        let Some(remote) = self.repo.remote.clone() else {
            return;
        };
        repo_options::spawn_repo_options_load(
            self.config.clone(),
            self.cwd.clone(),
            remote,
            force_refresh,
            self.bg_tx.clone(),
        );
    }

    fn spawn_pull_requests_load(&mut self, force_refresh: bool) {
        if self.pull_requests.is_loading() && !force_refresh {
            return;
        }

        let request_id = self.pull_requests.begin_load();
        tea::spawn_pull_requests_load(
            self.config.clone(),
            self.cwd.clone(),
            request_id,
            self.bg_tx.clone(),
        );
    }

    fn apply_repo(&mut self, repo: RepoState) {
        let was_no_remote = self.repo.remote.is_none();
        let has_remote_now = repo.remote.is_some();
        let inside_workspace = repo.inside_workspace;
        self.repo = repo;

        if self.screen == Screen::Generate && !inside_workspace {
            self.log("Generate PR blocked: cwd is not inside a jj workspace");
            self.screen = Screen::Landing;
            self.focus = Focus::Menu;
            self.input_mode = InputMode::Normal;
        }

        // If a remote just became available, start the initial repo options load.
        if was_no_remote && has_remote_now {
            self.spawn_repo_options_load(false);
        }
    }

    fn apply_repo_options(&mut self, result: RepoOptionsResult) {
        if let Some(warning) = result.options.status_warning()
            && !result.from_cache
        {
            self.log(format!("repo picker options: {warning}"));
        }

        let source = if result.from_cache {
            "cache"
        } else {
            "live fetch"
        };
        let label_count = result.options.labels.len();
        let milestone_count = result.options.milestones.len();
        let assignee_count = result.options.assignees.len();
        self.log(format!(
            "repo picker options loaded from {source}: {label_count} labels, {milestone_count} milestones, {assignee_count} assignees"
        ));

        // Update picker options on the generate form without overwriting valid user selections.
        self.generate
            .form
            .labels
            .set_picker_options(result.options.label_picker_options());
        self.generate
            .form
            .assignees
            .set_picker_options(result.options.assignee_picker_options());
        self.generate
            .form
            .milestone
            .set_picker_options(result.options.milestone_picker_options());

        // Validate form now that picker options have changed.
        self.generate.validate_form();

        self.repo_options = result.options;
    }

    fn apply_pull_requests(&mut self, result: PullRequestsResult) {
        match result {
            PullRequestsResult::Ready { request_id, items } => {
                if !self.pull_requests.complete_load(request_id, items) {
                    self.log(format!(
                        "discarded PR list result {request_id}: newer request is active"
                    ));
                    return;
                }

                let count = self.pull_requests.items.len();
                self.log(format!("loaded {count} open pull requests"));
            }
            PullRequestsResult::Failed {
                request_id,
                command,
                message,
                stdout,
                stderr,
            } => {
                if !self.pull_requests.fail_request(request_id, message.clone()) {
                    self.log(format!(
                        "discarded PR list error {request_id}: newer request is active"
                    ));
                    return;
                }

                self.log(format!("PR list load failed: {message}"));
                self.log_command_capture("tea pr list", &command, &stdout, &stderr);
            }
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
            GenerationResult::Ready { request_id, draft } => {
                if !self.accept_generation_result(request_id) {
                    return;
                }
                self.log_raw_model_response(&draft.raw_model_response);
                let branch = draft.branch_name.clone();
                self.generate.complete_generation(draft);
                self.log(format!("llm generation finished for {branch}"));
            }
            GenerationResult::Failed { request_id, error } => {
                if !self.accept_generation_result(request_id) {
                    return;
                }
                if let Some(raw_response) = error.raw_response.as_ref() {
                    self.log_raw_model_response(raw_response);
                }
                let message = error.message.clone();
                self.generate.fail_generation(&message);
                self.log(format!("llm generation failed: {message}"));
            }
        }
    }

    fn accept_generation_result(&mut self, request_id: u64) -> bool {
        if self.screen != Screen::Generate || self.generate.phase != GeneratePhase::Generating {
            self.active_generation = None;
            self.log(format!(
                "discarded generation result {request_id}: generation is no longer active"
            ));
            return false;
        }

        let Some(active) = self.active_generation.take() else {
            self.log(format!(
                "discarded generation result {request_id}: no request is active"
            ));
            return false;
        };

        if active.id != request_id {
            self.active_generation = Some(active);
            self.log(format!(
                "discarded generation result {request_id}: newer request is active"
            ));
            return false;
        }

        let current_revset = self
            .generate
            .prompt()
            .map(|prompt| prompt.manifest.selected_revset.as_str())
            .unwrap_or_else(|| self.generate.form.head.display_value().trim());

        if active.selected_revset != current_revset || active.form != self.generate.form {
            self.generate
                .fail_generation("generation result is stale; form or revset changed");
            self.log(format!(
                "discarded generation result {request_id}: form or revset changed"
            ));
            return false;
        }

        true
    }

    fn apply_context(&mut self, context: ContextResult) {
        match context {
            ContextResult::Ready(bundle) => {
                let current_head = self.generate.form.head.display_value().trim();
                if bundle.repo_identity.selected_revset != current_head {
                    let stale = format!(
                        "discarded stale context for {}; selected revset is {}",
                        bundle.repo_identity.selected_revset, current_head
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
                    self.generate
                        .context
                        .as_ref()
                        .map(|context| &context.selected_revset)
                        .unwrap_or_else(|| self.generate.selected_revset()),
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

    pub fn generate_mut(&mut self) -> &mut GenerateState {
        &mut self.generate
    }

    pub fn pull_requests(&self) -> &PullRequestState {
        &self.pull_requests
    }

    pub fn pull_requests_mut(&mut self) -> &mut PullRequestState {
        &mut self.pull_requests
    }

    pub fn issues(&self) -> &ListState {
        &self.issues
    }

    pub fn issues_mut(&mut self) -> &mut ListState {
        &mut self.issues
    }

    pub fn logs(&self) -> &LogState {
        &self.logs
    }

    pub fn jobs(&self) -> &JobRegistry {
        &self.jobs
    }

    pub fn repo_options(&self) -> &RepoOptions {
        &self.repo_options
    }

    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
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

    fn sample_pull_request(index: u64, title: &str) -> PullRequestSummary {
        PullRequestSummary {
            index,
            title: title.into(),
            state: "open".into(),
            author: "alice".into(),
            url: format!("https://example.com/pr/{index}"),
            head: format!("feature/{index}"),
            base: "main".into(),
            body: format!("Body {index}"),
            updated: "2026-05-29T10:00:00Z".into(),
            labels: vec!["ui".into()],
        }
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
    fn pr_filter_edit_mode_routes_printable_keys_into_the_filter() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.focus = Focus::Form;

        app.update(Action::Edit);

        assert_eq!(app.input_mode, InputMode::Editing);
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
        assert_eq!(app.generate.form.title.display_value(), "x");
        assert!(!app.generate.form.title.display_value().contains('\n'));
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
        assert_eq!(app.generate.form.description.display_value(), "x\ny");
    }

    #[test]
    fn draft_ready_maps_c_to_confirm_execution() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::DraftReady;
        app.focus = Focus::Preview;

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
        app.focus = Focus::Preview;

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
    fn stale_generation_result_does_not_overwrite_edited_form() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::Generating;
        app.active_generation = Some(GenerationRequest {
            id: 1,
            selected_revset: "@".into(),
            form: app.generate.form.clone(),
        });
        app.generate.form.title = crate::generate::FieldState::new("User edit");

        app.apply_generation(GenerationResult::Ready {
            request_id: 1,
            draft: crate::generate::GeneratedDraft {
                branch_name: "feature/generated".into(),
                title: "Generated title".into(),
                body: "Generated body".into(),
                review_notes: Vec::new(),
                raw_model_response: "{}".into(),
            },
        });

        assert_eq!(app.generate.phase, GeneratePhase::Failed);
        assert!(app.generate.draft.is_none());
        assert_eq!(app.generate.form.title.display_value(), "User edit");
    }

    #[test]
    fn pr_filter_clamps_selection_when_filter_changes() {
        let mut app = test_app();
        app.pull_requests.items = vec![
            sample_pull_request(1, "First"),
            sample_pull_request(2, "Second"),
        ];
        app.pull_requests.selected_item = 1;
        app.pull_requests.filter = crate::generate::TextFieldState::new("Second");

        app.pull_requests.commit_filter();

        assert_eq!(app.pull_requests.selected_item, 0);
        assert_eq!(app.pull_requests.visible_count(), 1);
        assert_eq!(
            app.pull_requests
                .selected_item()
                .expect("selected PR")
                .title,
            "Second"
        );
    }

    #[test]
    fn stale_pr_list_result_is_ignored() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        let request_id = app.pull_requests.begin_load();

        app.handle_background(BackgroundEvent::PullRequests(PullRequestsResult::Ready {
            request_id: request_id + 1,
            items: vec![sample_pull_request(1, "Fresh")],
        }));

        assert!(app.pull_requests.items.is_empty());
        assert_eq!(
            app.pull_requests.load_status,
            PullRequestLoadStatus::Loading
        );
        assert_eq!(app.pull_requests.active_request_id, Some(request_id));
    }

    #[test]
    fn confirmation_plan_uses_context_revset_for_existing_bookmarks() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.form = crate::generate::PrForm::new("custom-head", "feature/existing", "main");
        app.generate.form.title = crate::generate::FieldState::new("Create a PR");
        app.generate.form.description = crate::generate::FieldState::new("Body");
        app.generate.context = Some(crate::context::ContextBundle {
            repo_identity: crate::context::RepoIdentity {
                collected_at: std::time::SystemTime::now(),
                workspace_root: Some(std::path::PathBuf::from("C:/repo")),
                remote_url: None,
                base_branch: "main".into(),
                selected_revset: "custom-head".into(),
            },
            remote: None,
            form: app.generate.form.clone(),
            selected_revset: crate::generate::RevsetSummary::new(
                "custom-head",
                "description",
                vec!["feature/existing".into()],
                "1 file changed",
                1,
                vec!["abc123".into()],
                vec!["custom-head".into()],
                Vec::new(),
                Vec::new(),
            ),
            selected_descriptions: Vec::new(),
            status: crate::context::CommandCapture::new("jj status", String::new(), String::new()),
            revset_log: crate::context::CommandCapture::new("jj log", String::new(), String::new()),
            diff_stats: crate::context::CommandCapture::new(
                "jj diff --stat",
                String::new(),
                String::new(),
            ),
            diff: crate::context::CommandCapture::new("jj diff", String::new(), String::new()),
        });
        app.generate.begin_confirmation_check();

        app.apply_stale_check(StaleCheckResult::Fresh);

        let plan = app
            .generate
            .execution_plan
            .as_ref()
            .expect("execution plan");
        assert_eq!(plan.steps[0].command.args[2], "move");
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

    /// Gate: discovering=true, inside_workspace=false — Enter on Generate PR stays on Landing.
    #[test]
    fn landing_entry_stays_on_landing_while_discovering() {
        let mut app = test_app();
        app.repo.discovering = true;
        app.repo.inside_workspace = false;
        app.landing.selected_entry = 0;

        app.open_selected_landing_entry();

        assert_eq!(app.screen, Screen::Landing);
    }

    /// Gate: discovering=false, inside_workspace=false — Enter on Generate PR stays on Landing.
    #[test]
    fn landing_entry_stays_on_landing_when_not_in_workspace() {
        let mut app = test_app();
        app.repo.discovering = false;
        app.repo.inside_workspace = false;
        app.landing.selected_entry = 0;

        app.open_selected_landing_entry();

        assert_eq!(app.screen, Screen::Landing);
    }

    /// Gate: discovering=false, inside_workspace=true — Enter on Generate PR transitions to Generate.
    #[test]
    fn landing_entry_transitions_to_generate_when_workspace_ready() {
        let mut app = test_app();
        app.repo.discovering = false;
        app.repo.inside_workspace = true;
        app.landing.selected_entry = 0;

        app.open_selected_landing_entry();

        assert_eq!(app.screen, Screen::Generate);
    }

    #[test]
    fn preview_navigation_scrolls_without_moving_the_selected_revset() {
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.focus = Focus::Preview;
        app.generate.selected_revset = 1;

        app.update(Action::Navigate(Direction::Down));

        assert_eq!(app.generate.selected_revset, 1);
        assert_eq!(app.generate.preview_scroll.offset, 1);

        app.update(Action::Navigate(Direction::Up));

        assert_eq!(app.generate.selected_revset, 1);
        assert_eq!(app.generate.preview_scroll.offset, 0);
    }

    // -------------------------------------------------------------------------
    // Pane-local keymap tests (Generate screen)
    // -------------------------------------------------------------------------

    #[test]
    fn p_is_noop_outside_preview_focus() {
        // 'p' should be a no-op (Tick) when any non-Preview pane is focused.
        let mut app = test_app();
        app.screen = Screen::Generate;

        app.focus = Focus::Menu;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::empty())),
            Action::Tick,
            "'p' must be a no-op on Menu pane"
        );

        app.focus = Focus::Form;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::empty())),
            Action::Tick,
            "'p' must be a no-op on Form pane"
        );
    }

    #[test]
    fn global_keys_work_from_all_generate_panes() {
        // Tab, Esc, q, arrows must work regardless of which pane is focused.
        for focus in [Focus::Menu, Focus::Form, Focus::Preview] {
            let mut app = test_app();
            app.screen = Screen::Generate;
            app.focus = focus;

            assert_eq!(
                app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())),
                Action::FocusNext,
                "Tab must work from {focus:?}"
            );
            assert_eq!(
                app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
                Action::Back,
                "Esc must work from {focus:?}"
            );
            assert_eq!(
                app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty())),
                Action::Quit,
                "'q' must work from {focus:?}"
            );
        }
    }

    // -------------------------------------------------------------------------
    // PR comment modal tests
    // -------------------------------------------------------------------------

    #[test]
    fn c_key_opens_comment_modal_when_pr_is_selected() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.items = vec![sample_pull_request(1, "First")];
        app.pull_requests.selected_item = 0;

        let action = app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()));
        assert_eq!(action, Action::OpenCommentModal);

        app.update(action);
        assert_eq!(app.pull_requests.comment_phase, PrCommentPhase::Editing);
    }

    #[test]
    fn c_key_does_not_open_comment_modal_when_no_pr_selected() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        // No items — visible list is empty.

        let action = app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()));
        assert_eq!(action, Action::Tick);
        assert_eq!(app.pull_requests.comment_phase, PrCommentPhase::Idle);
    }

    #[test]
    fn comment_modal_captures_global_keys_in_editing_phase() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.items = vec![sample_pull_request(5, "PR Five")];
        app.pull_requests.comment_phase = PrCommentPhase::Editing;

        // 'q' should NOT quit — it should insert text.
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()));
        assert_eq!(
            action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()))
        );

        // 'j' should NOT navigate — it should insert text.
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()));
        assert_eq!(
            action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()))
        );

        // 'g' should NOT generate — it should insert text.
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty()));
        assert_eq!(
            action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty()))
        );

        // 'r' should NOT refresh — it should insert text.
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::empty()));
        assert_eq!(
            action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::empty()))
        );

        // 'c' should NOT open another modal — it should insert text.
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()));
        assert_eq!(
            action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn comment_modal_swallows_keys_while_submitting() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.comment_phase = PrCommentPhase::Submitting;

        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()),
        ] {
            assert_eq!(app.handle_key(key), Action::Tick);
        }
    }

    #[test]
    fn comment_modal_enter_submits_esc_cancels() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.comment_phase = PrCommentPhase::Editing;

        let enter_action = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(enter_action, Action::SubmitComment);

        let esc_action = app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert_eq!(esc_action, Action::CancelComment);
    }

    #[test]
    fn empty_comment_does_not_spawn_command_and_shows_error() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.items = vec![sample_pull_request(3, "Some PR")];
        app.pull_requests.comment_phase = PrCommentPhase::Editing;
        // Buffer is empty by default.

        app.submit_comment();

        // Should stay in Editing with an error, not move to Submitting.
        assert_eq!(app.pull_requests.comment_phase, PrCommentPhase::Editing);
        assert!(app.pull_requests.comment_error.is_some());
        assert!(
            app.pull_requests
                .comment_error
                .as_deref()
                .unwrap()
                .to_lowercase()
                .contains("empty")
        );
    }

    #[test]
    fn cancel_comment_clears_buffer_and_returns_to_idle() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.comment_phase = PrCommentPhase::Editing;
        app.pull_requests.comment_buffer = "partial text".into();
        app.pull_requests.comment_cursor = 12;

        app.update(Action::CancelComment);

        assert_eq!(app.pull_requests.comment_phase, PrCommentPhase::Idle);
        assert!(app.pull_requests.comment_buffer.is_empty());
        assert_eq!(app.pull_requests.comment_cursor, 0);
    }

    #[test]
    fn failed_comment_keeps_buffer_for_retry() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.comment_phase = PrCommentPhase::Submitting;
        app.pull_requests.comment_buffer = "my comment text".into();

        app.apply_pr_comment(crate::event::PrCommentResult::Failed {
            pr_index: 1,
            command: "tea comment 1 ...".into(),
            message: "connection refused".into(),
            stdout: String::new(),
            stderr: String::new(),
        });

        assert_eq!(app.pull_requests.comment_phase, PrCommentPhase::Failed);
        // Buffer must be preserved for retry.
        assert_eq!(app.pull_requests.comment_buffer, "my comment text");
        assert!(app.pull_requests.comment_error.is_some());
    }

    #[test]
    fn successful_comment_clears_buffer_and_closes_modal() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.comment_phase = PrCommentPhase::Submitting;
        app.pull_requests.comment_buffer = "LGTM".into();

        app.apply_pr_comment(crate::event::PrCommentResult::Succeeded {
            pr_index: 2,
            command: "tea comment 2 LGTM".into(),
            stdout: String::new(),
            stderr: String::new(),
        });

        assert_eq!(app.pull_requests.comment_phase, PrCommentPhase::Idle);
        assert!(app.pull_requests.comment_buffer.is_empty());
        assert!(app.pull_requests.comment_error.is_none());
    }

    #[test]
    fn generate_pr_c_confirm_behavior_not_regressed() {
        // DraftReady phase: 'c' maps to ConfirmExecution only when Preview is focused.
        let mut app = test_app();
        app.screen = Screen::Generate;
        app.generate.phase = GeneratePhase::DraftReady;
        app.focus = Focus::Preview;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty())),
            Action::ConfirmExecution
        );

        // Failed phase: 'c' maps to ConfirmExecution only when Preview is focused.
        app.generate.phase = GeneratePhase::Failed;
        app.focus = Focus::Preview;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty())),
            Action::ConfirmExecution
        );
    }

    // -------------------------------------------------------------------------
    // Open-in-browser / yank-URL shortcut tests
    // -------------------------------------------------------------------------

    fn pr_app_with_selected() -> App {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.items = vec![sample_pull_request(1, "First")];
        app.pull_requests.selected_item = 0;
        app
    }

    #[test]
    fn o_in_pr_view_emits_open_action_when_pr_selected() {
        let app = pr_app_with_selected();
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty()));
        assert_eq!(action, Action::OpenPrInBrowser);
    }

    #[test]
    fn y_in_pr_view_emits_copy_action_when_pr_selected() {
        let app = pr_app_with_selected();
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()));
        assert_eq!(action, Action::CopyPrUrl);
    }

    #[test]
    fn o_and_y_are_inert_with_no_selection() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        // No items — visible list is empty, selected_item() returns None.

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty())),
            Action::Tick
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty())),
            Action::Tick
        );
    }

    #[test]
    fn o_and_y_are_inert_while_comment_modal_open() {
        let mut app = pr_app_with_selected();
        app.pull_requests.comment_phase = PrCommentPhase::Editing;

        // While comment modal is open all keys are routed through
        // handle_comment_modal_key, so 'o' and 'y' become EditKey actions.
        let o_action = app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty()));
        assert_ne!(o_action, Action::OpenPrInBrowser);

        let y_action = app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()));
        assert_ne!(y_action, Action::CopyPrUrl);
    }

    #[test]
    fn o_and_y_are_inert_in_filter_edit_mode() {
        let mut app = pr_app_with_selected();
        app.input_mode = InputMode::Editing;

        // In Editing mode handle_edit_key routes characters to EditKey.
        let o_action = app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty()));
        assert_eq!(
            o_action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty()))
        );

        let y_action = app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()));
        assert_eq!(
            y_action,
            Action::EditKey(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn open_action_with_empty_url_logs_error_and_does_not_call_helper() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        // Insert a PR with an empty URL.
        app.pull_requests.items = vec![PullRequestSummary {
            index: 99,
            title: "No URL".into(),
            state: "open".into(),
            author: "bot".into(),
            url: "".into(),
            head: "branch".into(),
            base: "main".into(),
            body: "".into(),
            updated: "2026-05-29T00:00:00Z".into(),
            labels: vec![],
        }];
        app.pull_requests.selected_item = 0;

        // Dispatch OpenPrInBrowser directly (no OS call happens because the
        // guard catches the empty URL before reaching the external helper).
        app.update(Action::OpenPrInBrowser);

        assert!(
            app.status_message
                .as_deref()
                .unwrap_or("")
                .starts_with("error:"),
            "status_message should start with 'error:'"
        );
        assert!(
            app.logs.entries.iter().any(|e| e.contains("no URL")),
            "log should mention missing URL"
        );
    }

    #[test]
    fn copy_action_with_empty_url_logs_error_and_does_not_call_helper() {
        let mut app = test_app();
        app.screen = Screen::PullRequests;
        app.pull_requests.items = vec![PullRequestSummary {
            index: 100,
            title: "No URL either".into(),
            state: "open".into(),
            author: "bot".into(),
            url: "  ".into(), // whitespace only — should be treated as empty
            head: "branch".into(),
            base: "main".into(),
            body: "".into(),
            updated: "2026-05-29T00:00:00Z".into(),
            labels: vec![],
        }];
        app.pull_requests.selected_item = 0;

        app.update(Action::CopyPrUrl);

        assert!(
            app.status_message
                .as_deref()
                .unwrap_or("")
                .starts_with("error:"),
            "status_message should start with 'error:'"
        );
    }

    #[test]
    fn status_message_is_cleared_on_next_action() {
        let mut app = pr_app_with_selected();
        app.status_message = Some("opened https://example.com in browser".into());

        // Any action clears the message.
        app.update(Action::Tick);

        assert!(app.status_message.is_none());
    }

    #[test]
    fn comment_buffer_editing_inserts_and_moves_cursor() {
        let mut state = PullRequestState {
            comment_phase: PrCommentPhase::Editing,
            ..PullRequestState::default()
        };

        state.comment_input_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
        state.comment_input_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::empty()));
        state.comment_input_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()));

        assert_eq!(state.comment_buffer, "abc");
        assert_eq!(state.comment_cursor, 3);

        state.comment_input_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        assert_eq!(state.comment_cursor, 2);

        state.comment_input_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(state.comment_buffer, "ac");
        assert_eq!(state.comment_cursor, 1);

        state.comment_input_key(KeyEvent::new(KeyCode::Home, KeyModifiers::empty()));
        assert_eq!(state.comment_cursor, 0);

        // Delete at cursor 0 removes 'a'.
        state.comment_input_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()));
        assert_eq!(state.comment_buffer, "c");
        assert_eq!(state.comment_cursor, 0);

        state.comment_input_key(KeyEvent::new(KeyCode::End, KeyModifiers::empty()));
        assert_eq!(state.comment_cursor, 1);
    }
}
