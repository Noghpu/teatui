use std::any::Any;
use std::time::Duration;

use crossterm::event::KeyEvent;
use ratatui::Frame;

use crate::config::Config;
use crate::domain::{
    ContextJob, ContextResult, ExecutePrJob, ExecuteResult, LlmGenerateJob, LlmHealth,
    LlmHealthProbe, LlmResult, RevsetProbe, Revsets, StatusStore, TeaAuthProbe, TeaAuthStatus,
    VersionKind, VersionProbe, VersionResult, WorkspaceInfo, WorkspaceProbe, build_prompt, slugify,
};
use crate::input::InputEvent;
use crate::runtime::{JobEvent, JobOutcomeEvent, JobSubmitter};
use crate::screens::generate::GeneratePhase;
use crate::screens::{self, GenerateState, LandingState, NewScreen, Screen, Transition};

const CONTEXT_DIFF_BUDGET_BYTES: usize = 32 * 1024;

pub struct App {
    config: Config,
    submitter: JobSubmitter,
    status: StatusStore,
    screen: Screen,
    quit: bool,
    dirty: bool,
}

impl App {
    pub fn new(config: Config, submitter: JobSubmitter) -> Self {
        let mut status = StatusStore::new();
        status.mark_all_loading();

        let timeout = Duration::from_secs(config.llm.timeout_secs);
        submitter.submit(VersionProbe {
            kind: VersionKind::Jj,
            binary: config.commands.jj.clone(),
        });
        submitter.submit(VersionProbe {
            kind: VersionKind::Git,
            binary: config.commands.git.clone(),
        });
        submitter.submit(VersionProbe {
            kind: VersionKind::Tea,
            binary: config.commands.tea.clone(),
        });
        submitter.submit(WorkspaceProbe {
            jj_binary: config.commands.jj.clone(),
        });
        submitter.submit(TeaAuthProbe {
            tea_binary: config.commands.tea.clone(),
        });
        submitter.submit(LlmHealthProbe {
            base_url: config.llm.base_url.clone(),
            timeout,
        });
        submitter.submit(RevsetProbe {
            jj_binary: config.commands.jj.clone(),
            revset: "mutable()".into(),
        });

        Self {
            config,
            submitter,
            status,
            screen: Screen::default(),
            quit: false,
            dirty: true,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn on_input(&mut self, event: InputEvent) {
        match event {
            InputEvent::Key(key) => self.dispatch_key(key),
            InputEvent::Resize { .. } => self.dirty = true,
        }
    }

    fn dispatch_key(&mut self, key: KeyEvent) {
        let transition = match &mut self.screen {
            Screen::Landing(state) => screens::landing::on_key(state, key),
            Screen::Generate(state) => screens::generate::on_key(state, &self.status, key),
        };
        self.apply_transition(transition);
    }

    fn apply_transition(&mut self, t: Transition) {
        match t {
            Transition::None => {}
            Transition::Dirty => self.dirty = true,
            Transition::Quit => {
                tracing::info!(target: "teatui::lifecycle", "quit requested");
                self.quit = true;
            }
            Transition::Navigate(target) => {
                self.screen = match target {
                    NewScreen::Landing => Screen::Landing(LandingState::default()),
                    NewScreen::Generate => {
                        let mut state = GenerateState::new(self.config.pr.default_base.clone());
                        if let Some(Revsets::Loaded(items)) = self.status.revsets.value()
                            && let Some(first) = items.first()
                        {
                            state.form.head = first.change_id.clone();
                        }
                        Screen::Generate(Box::new(state))
                    }
                };
                tracing::info!(target: "teatui::lifecycle", screen = screen_label(&self.screen), "navigated");
                self.dirty = true;
            }
            Transition::Generate => self.start_generation(),
            Transition::Execute => self.start_execution(),
            Transition::CopyUrl => self.copy_done_url(),
            Transition::OpenUrl => self.open_done_url(),
        }
    }

    fn start_generation(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        if state.form.head.is_empty() {
            tracing::warn!(target: "teatui::generate", "ignored: no head");
            return;
        }
        if state.is_in_progress() {
            tracing::debug!(target: "teatui::generate", "ignored: already in progress");
            return;
        }
        state.last_action = None;
        state.phase = GeneratePhase::Collecting;
        self.dirty = true;
        let head = state.form.head.clone();
        self.submitter.submit(ContextJob {
            jj_binary: self.config.commands.jj.clone(),
            revset: head.clone(),
            diff_byte_budget: CONTEXT_DIFF_BUDGET_BYTES,
        });
        tracing::info!(target: "teatui::generate", revset = %head, "started");
    }

    fn start_execution(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let phase = std::mem::replace(&mut state.phase, GeneratePhase::Idle);
        let draft = match phase {
            GeneratePhase::DraftReady { draft, .. } => draft,
            other => {
                state.phase = other;
                tracing::debug!(target: "teatui::execute", "ignored: not in DraftReady");
                return;
            }
        };
        let form = state.form.clone();
        if form.head.is_empty() || form.branch.is_empty() || form.title.is_empty() {
            state.phase = GeneratePhase::Failed {
                message: "missing head, branch, or title".into(),
            };
            self.dirty = true;
            return;
        }
        let job = ExecutePrJob {
            jj_binary: self.config.commands.jj.clone(),
            tea_binary: self.config.commands.tea.clone(),
            change_id: form.head,
            bookmark: form.branch,
            base: form.base,
            title: form.title,
            description: form.description,
        };
        state.phase = GeneratePhase::Executing { draft };
        state.last_action = None;
        self.dirty = true;
        self.submitter.submit(job);
        tracing::info!(target: "teatui::execute", "submitted");
    }

    fn copy_done_url(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let Some(url) = state.done_url().map(str::to_string) else {
            return;
        };
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(url.clone())) {
            Ok(()) => {
                state.last_action = Some("copied to clipboard");
                tracing::info!(target: "teatui::execute", %url, "copied URL");
            }
            Err(e) => {
                tracing::error!(target: "teatui::execute", error = %e, "clipboard write failed");
                state.last_action = Some("clipboard write failed");
            }
        }
        self.dirty = true;
    }

    fn open_done_url(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let Some(url) = state.done_url().map(str::to_string) else {
            return;
        };
        match opener::open(&url) {
            Ok(()) => {
                state.last_action = Some("opened in browser");
                tracing::info!(target: "teatui::execute", %url, "opened URL");
            }
            Err(e) => {
                tracing::error!(target: "teatui::execute", error = %e, "open failed");
                state.last_action = Some("open failed");
            }
        }
        self.dirty = true;
    }

    pub fn on_job(&mut self, event: JobEvent) {
        let JobEvent { name, outcome, .. } = event;
        match outcome {
            JobOutcomeEvent::Failed(msg) => {
                tracing::error!(target: "teatui::jobs", name, %msg, "failed");
            }
            JobOutcomeEvent::Done(any) => self.absorb_payload(name, any),
        }
    }

    fn absorb_payload(&mut self, name: &'static str, any: Box<dyn Any + Send>) {
        let any = match any.downcast::<VersionResult>() {
            Ok(b) => {
                self.status.set_version(*b);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<WorkspaceInfo>() {
            Ok(b) => {
                self.status.set_workspace(*b);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<TeaAuthStatus>() {
            Ok(b) => {
                self.status.set_tea_auth(*b);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<LlmHealth>() {
            Ok(b) => {
                self.status.set_llm(*b);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<Revsets>() {
            Ok(b) => {
                self.status.set_revsets(*b);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<ContextResult>() {
            Ok(b) => {
                self.handle_context_result(*b);
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<LlmResult>() {
            Ok(b) => {
                self.handle_llm_result(*b);
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<ExecuteResult>() {
            Ok(b) => {
                self.handle_execute_result(*b);
                return;
            }
            Err(a) => a,
        };
        let _ = any;
        tracing::warn!(target: "teatui::jobs", name, "unhandled payload type");
    }

    fn handle_context_result(&mut self, result: ContextResult) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        if !matches!(state.phase, GeneratePhase::Collecting) {
            tracing::debug!(target: "teatui::generate", "stale context result ignored");
            return;
        }
        match result {
            ContextResult::Ready(bundle) => {
                let prompt = build_prompt(&bundle);
                let llm_job = LlmGenerateJob {
                    base_url: self.config.llm.base_url.clone(),
                    model: self.config.llm.model.clone(),
                    prompt: prompt.prompt.clone(),
                    temperature: self.config.llm.temperature,
                    max_tokens: self.config.llm.max_tokens,
                    timeout: Duration::from_secs(self.config.llm.timeout_secs),
                };
                state.phase = GeneratePhase::Generating {
                    context: bundle,
                    prompt,
                };
                self.submitter.submit(llm_job);
            }
            ContextResult::Errored { message } => {
                tracing::error!(target: "teatui::generate", %message, "context failed");
                state.phase = GeneratePhase::Failed { message };
            }
        }
        self.dirty = true;
    }

    fn handle_llm_result(&mut self, result: LlmResult) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let phase = std::mem::replace(&mut state.phase, GeneratePhase::Idle);
        let prompt = match phase {
            GeneratePhase::Generating { prompt, .. } => prompt,
            other => {
                tracing::debug!(target: "teatui::generate", "stale llm result ignored");
                state.phase = other;
                return;
            }
        };
        match result {
            LlmResult::Ready(draft) => {
                state.form.title = draft.title.clone();
                state.form.description = draft.description.clone();
                if state.form.branch.is_empty() {
                    let slug = slugify(&draft.title);
                    if !slug.is_empty() {
                        state.form.branch = slug;
                    }
                }
                state.phase = GeneratePhase::DraftReady { draft, prompt };
            }
            LlmResult::Errored { message } => {
                tracing::error!(target: "teatui::generate", %message, "llm failed");
                state.phase = GeneratePhase::Failed { message };
            }
        }
        self.dirty = true;
    }

    fn handle_execute_result(&mut self, result: ExecuteResult) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        if !matches!(state.phase, GeneratePhase::Executing { .. }) {
            tracing::debug!(target: "teatui::execute", "stale execute result ignored");
            return;
        }
        match result {
            ExecuteResult::Ready { url } => {
                tracing::info!(target: "teatui::execute", %url, "succeeded");
                state.phase = GeneratePhase::Done { url };
            }
            ExecuteResult::Errored { step, message } => {
                let full = format!("{}: {message}", step.label());
                tracing::error!(target: "teatui::execute", step = step.label(), %message, "failed");
                state.phase = GeneratePhase::Failed { message: full };
            }
        }
        self.dirty = true;
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        match &self.screen {
            Screen::Landing(state) => screens::landing::render(state, &self.status, frame, area),
            Screen::Generate(state) => screens::generate::render(state, &self.status, frame, area),
        }
    }
}

fn screen_label(screen: &Screen) -> &'static str {
    match screen {
        Screen::Landing(_) => "landing",
        Screen::Generate(_) => "generate",
    }
}
