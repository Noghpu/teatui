use std::any::Any;
use std::time::Duration;

use crossterm::event::KeyEvent;
use ratatui::Frame;

use crate::config::Config;
use crate::domain::{
    BackendHealth, BackendHealthProbe, BaseBookmarks, BaseBookmarksProbe, ContextJob,
    ContextResult, ExecutePrJob, ExecuteResult, GeneratedDraft, LlmGenerateJob, LlmResult,
    PromptForm, RepoOptions, RepoOptionsProbe, RevsetProbe, RevsetStats, RevsetStatsProbe, Revsets,
    StatusStore, TeaAuthProbe, TeaAuthStatus, VersionKind, VersionProbe, VersionResult,
    WorkspaceInfo, WorkspaceProbe, build_prompt, slugify,
};
use crate::input::InputEvent;
use crate::runtime::{CancelHandle, JobEvent, JobOutcomeEvent, JobSubmitter};
use crate::screens::backend_picker::{BackendPicker, PickerOutcome};
use crate::screens::generate::{GeneratePhase, PrForm};
use crate::screens::{self, GenerateState, LandingState, NewScreen, Screen, Transition};

/// Byte budget for the single aggregate diff sent to the LLM. Per-change diffs
/// are no longer sent, so the whole budget now backs one diff (was split across
/// every change before); raised accordingly to keep the destination diff intact.
const CONTEXT_DIFF_BUDGET_BYTES: usize = 64 * 1024;

/// Health probes must be snappy — they fan out across every backend when
/// the switcher opens, and a dead backend should resolve to ✗ quickly
/// rather than hang for the (much longer) generation timeout.
const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub struct App {
    config: Config,
    submitter: JobSubmitter,
    status: StatusStore,
    screen: Screen,
    /// LLM backend switcher overlay, shown over any screen when `Some`.
    backend_picker: Option<BackendPicker>,
    /// Aborts the in-flight LLM generation request. `Some` only while a
    /// generation job is running; taken when the result lands or the user
    /// cancels. See [`App::cancel_generation`].
    gen_cancel: Option<CancelHandle>,
    quit: bool,
    dirty: bool,
}

impl App {
    pub fn new(config: Config, submitter: JobSubmitter) -> Self {
        let mut status = StatusStore::new();
        status.mark_all_loading();

        let llm = config.llm.active_backend().clone();
        status.mark_backend_loading(&llm.name);
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
        submitter.submit(BackendHealthProbe {
            name: llm.name.clone(),
            base_url: llm.base_url.clone(),
            api: llm.api,
            api_key: llm.api_key.clone(),
            timeout: HEALTH_PROBE_TIMEOUT,
        });
        submitter.submit(RevsetProbe {
            jj_binary: config.commands.jj.clone(),
            revset: "trunk()..@".into(),
        });
        submitter.submit(BaseBookmarksProbe {
            jj_binary: config.commands.jj.clone(),
        });

        Self {
            config,
            submitter,
            status,
            screen: Screen::default(),
            backend_picker: None,
            gen_cancel: None,
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
        // The backend switcher is a modal overlay: while it's open it eats
        // all keys so the screen underneath never sees them.
        if self.backend_picker.is_some() {
            self.dispatch_backend_picker_key(key);
            return;
        }
        let transition = match &mut self.screen {
            Screen::Landing(state) => screens::landing::on_key(state, key),
            Screen::Generate(state) => screens::generate::on_key(state, &self.status, key),
        };
        self.apply_transition(transition);
    }

    fn dispatch_backend_picker_key(&mut self, key: KeyEvent) {
        let Some(picker) = &mut self.backend_picker else {
            return;
        };
        match picker.on_key(key, self.config.llm.backends.len()) {
            PickerOutcome::None => {}
            PickerOutcome::Dirty => self.dirty = true,
            PickerOutcome::Close => {
                self.backend_picker = None;
                self.dirty = true;
            }
            PickerOutcome::Select(index) => {
                if let Some(backend) = self.config.llm.backends.get(index) {
                    let name = backend.name.clone();
                    tracing::info!(target: "teatui::llm", backend = %name, "switched active backend");
                    // Session-only: flip the in-memory active name. Generation
                    // reads `active_backend()` at submit time, so this takes
                    // effect on the next generate without re-wiring anything.
                    self.config.llm.active = name.clone();
                    // The landing chip reads `status.llm`, which only tracks the
                    // active backend. Sync it from the just-probed per-backend
                    // health so the chip reflects the new backend immediately
                    // instead of waiting for the picker to be reopened.
                    if let Some(cached) = self.status.backend_health(&name) {
                        self.status.llm = cached.clone();
                    } else {
                        self.status.llm.mark_loading();
                    }
                }
                self.backend_picker = None;
                self.dirty = true;
            }
        }
    }

    fn open_backend_picker(&mut self) {
        let active = self.config.llm.active_backend().name.clone();
        self.backend_picker = Some(BackendPicker::new(&active, &self.config.llm.backends));
        // Probe every backend so each row resolves to ✓/✗. Known rows keep
        // their symbol (Stale) while re-probing; never-probed rows show the
        // pending glyph until their result lands.
        let targets: Vec<_> = self
            .config
            .llm
            .backends
            .iter()
            .map(|b| (b.name.clone(), b.base_url.clone(), b.api, b.api_key.clone()))
            .collect();
        for (name, base_url, api, api_key) in targets {
            self.status.mark_backend_loading(&name);
            self.submitter.submit(BackendHealthProbe {
                name,
                base_url,
                api,
                api_key,
                timeout: HEALTH_PROBE_TIMEOUT,
            });
        }
        self.dirty = true;
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
                            state.form.head.set_value(first.change_id.clone());
                        }
                        state.ensure_field_options_synced(&self.status);
                        Screen::Generate(Box::new(state))
                    }
                };
                tracing::info!(target: "teatui::lifecycle", screen = screen_label(&self.screen), "navigated");
                self.dirty = true;
            }
            Transition::Generate => self.start_generation(),
            Transition::CancelGeneration => self.cancel_generation(),
            Transition::Execute => self.start_execution(),
            Transition::CopyUrl => self.copy_done_url(),
            Transition::OpenUrl => self.open_done_url(),
            Transition::RefreshRevsets => {
                self.status.revsets.mark_loading();
                self.submitter.submit(RevsetProbe {
                    jj_binary: self.config.commands.jj.clone(),
                    revset: "trunk()..@".into(),
                });
                self.dirty = true;
            }
            Transition::OpenBackendPicker => self.open_backend_picker(),
        }
    }

    fn start_generation(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let base = state.form.base().to_string();
        let head = state.form.head().to_string();
        if base.is_empty() || head.is_empty() {
            tracing::warn!(target: "teatui::generate", "ignored: missing base or head");
            return;
        }
        if state.is_in_progress() {
            tracing::debug!(target: "teatui::generate", "ignored: already in progress");
            return;
        }
        state.last_action = None;
        state.phase = GeneratePhase::Collecting;
        self.dirty = true;
        self.submitter.submit(ContextJob {
            jj_binary: self.config.commands.jj.clone(),
            base: base.clone(),
            head: head.clone(),
            diff_byte_budget: CONTEXT_DIFF_BUDGET_BYTES,
        });
        tracing::info!(target: "teatui::generate", base = %base, head = %head, "started");
    }

    /// Abort an in-flight generation. Shuts down the LLM request socket (freeing
    /// the server slot) if one is live, then returns to idle and acknowledges
    /// the cancel. The abandoned `ContextResult`/`LlmResult` still arrives later
    /// but is discarded by the stale-phase guards in its handler.
    ///
    /// Scoped to the LLM phases. `Collecting` has no socket to abort yet — its
    /// jj subprocess (~1–2s) is left to finish — but we still drop to idle so
    /// the user can edit and regenerate. `Executing` is intentionally excluded
    /// (interrupting a half-created PR is worse than waiting it out).
    fn cancel_generation(&mut self) {
        if let Some(cancel) = self.gen_cancel.take() {
            cancel.cancel();
        }
        if let Screen::Generate(state) = &mut self.screen
            && matches!(
                state.phase,
                GeneratePhase::Collecting | GeneratePhase::Generating { .. }
            )
        {
            state.phase = GeneratePhase::Idle;
            state.last_action = Some("generation cancelled");
            tracing::info!(target: "teatui::generate", "cancelled by user");
        }
        self.dirty = true;
    }

    fn start_execution(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let phase = std::mem::replace(&mut state.phase, GeneratePhase::Idle);
        let draft = match phase {
            GeneratePhase::Confirming { draft, .. } => draft,
            other => {
                state.phase = other;
                tracing::debug!(target: "teatui::execute", "ignored: not confirming");
                return;
            }
        };
        if !state.form.validate() {
            state.phase = GeneratePhase::Failed {
                message: "form has validation errors".into(),
            };
            self.dirty = true;
            return;
        }
        let job = ExecutePrJob {
            jj_binary: self.config.commands.jj.clone(),
            tea_binary: self.config.commands.tea.clone(),
            change_id: state.form.head().to_string(),
            bookmark: state.form.branch().to_string(),
            base: state.form.base().to_string(),
            title: state.form.title().to_string(),
            description: state.form.description().to_string(),
            labels: state.form.labels(),
            assignees: state.form.assignees(),
            milestone: state.form.milestone().to_string(),
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
                let workspace = *b;
                if let WorkspaceInfo::Inside {
                    remote: Some(remote),
                    ..
                } = &workspace
                {
                    self.status.mark_repo_options_loading();
                    self.submitter.submit(RepoOptionsProbe {
                        tea_binary: self.config.commands.tea.clone(),
                        owner: remote.owner.clone(),
                        repo: remote.repo.clone(),
                    });
                }
                self.status.set_workspace(workspace);
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
        let any = match any.downcast::<BackendHealth>() {
            Ok(b) => {
                let BackendHealth { name, health } = *b;
                // The active backend also drives the landing LLM chip.
                if name == self.config.llm.active_backend().name {
                    self.status.set_llm(health.clone());
                }
                self.status.set_backend_health(name, health);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<Revsets>() {
            Ok(b) => {
                self.status.set_revsets(*b);
                self.sync_generate_options();
                // If the user opened PR-gen before revsets loaded, the
                // form's `head` field is still empty. Now that the list
                // is in, snap it to the currently-selected revset so the
                // Form pane reflects the visible Changes-pane cursor.
                if let Screen::Generate(state) = &mut self.screen
                    && state.form.head().is_empty()
                {
                    screens::generate::update_head_from_selection(state, &self.status);
                }
                // Kick off the deferred stats fetch. This is a heavier
                // jj subprocess (~1.4s on this workspace) that we keep
                // off the first-paint path; results merge in via the
                // RevsetStats handler below.
                self.submitter.submit(RevsetStatsProbe {
                    jj_binary: self.config.commands.jj.clone(),
                    revset: "trunk()..@".into(),
                });
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<RevsetStats>() {
            Ok(b) => {
                self.status.merge_revset_stats(*b);
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<BaseBookmarks>() {
            Ok(b) => {
                self.status.set_base_bookmarks(*b);
                self.sync_generate_options();
                self.dirty = true;
                return;
            }
            Err(a) => a,
        };
        let any = match any.downcast::<RepoOptions>() {
            Ok(b) => {
                self.status.set_repo_options(*b);
                self.sync_generate_options();
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
                let form = prompt_form_from_generate(&state.form);
                let prompt = build_prompt(&bundle, &form);
                let llm = self.config.llm.active_backend();
                let cancel = CancelHandle::new();
                let llm_job = LlmGenerateJob {
                    base_url: llm.base_url.clone(),
                    model: llm.model.clone(),
                    api: llm.api,
                    api_key: llm.api_key.clone(),
                    prompt: prompt.prompt.clone(),
                    temperature: llm.temperature,
                    max_tokens: llm.max_tokens,
                    timeout: Duration::from_secs(llm.timeout_secs),
                    cancel: cancel.clone(),
                };
                state.phase = GeneratePhase::Generating {
                    context: bundle,
                    prompt,
                };
                self.gen_cancel = Some(cancel);
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
        // The request has resolved (succeeded, failed, or its cancellation took
        // effect), so the abort handle is spent either way.
        self.gen_cancel = None;
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let phase = std::mem::replace(&mut state.phase, GeneratePhase::Idle);
        let prompt = match phase {
            GeneratePhase::Generating { prompt, .. } => prompt,
            other => {
                // A user cancel already returned the phase to Idle and
                // acknowledged it; the late Cancelled/result just lands here.
                tracing::debug!(target: "teatui::generate", "stale llm result ignored");
                state.phase = other;
                return;
            }
        };
        match result {
            LlmResult::Ready(draft) => {
                state.form.title.set_value(draft.title.clone());
                state.form.description.set_value(draft.description.clone());
                if state.form.branch().is_empty() {
                    let branch = branch_from_draft(&draft);
                    if !branch.is_empty() {
                        state.form.branch_name.set_value(branch);
                    }
                }
                state.phase = GeneratePhase::DraftReady { draft, prompt };
            }
            LlmResult::Errored { message } => {
                tracing::error!(target: "teatui::generate", %message, "llm failed");
                state.phase = GeneratePhase::Failed { message };
            }
            // The request was still `Generating` when its cancellation resolved
            // (e.g. an https backend the socket abort couldn't reach). Treat it
            // exactly like a user cancel: drop to idle, acknowledge, no error.
            LlmResult::Cancelled => {
                tracing::info!(target: "teatui::generate", "llm cancelled");
                state.phase = GeneratePhase::Idle;
                state.last_action = Some("generation cancelled");
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
        if let Some(picker) = &self.backend_picker {
            screens::backend_picker::render(
                picker,
                &self.config.llm.backends,
                &self.config.llm.active_backend().name,
                &self.status,
                frame,
                area,
            );
        }
    }

    fn sync_generate_options(&mut self) {
        if let Screen::Generate(state) = &mut self.screen {
            state.ensure_field_options_synced(&self.status);
        }
    }
}

fn screen_label(screen: &Screen) -> &'static str {
    match screen {
        Screen::Landing(_) => "landing",
        Screen::Generate(_) => "generate",
    }
}

fn prompt_form_from_generate(form: &PrForm) -> PromptForm {
    PromptForm {
        head: form.head().to_string(),
        base: form.base().to_string(),
        branch: form.branch().to_string(),
        title: form.title().to_string(),
        description: form.description().to_string(),
    }
}

fn branch_from_draft(draft: &GeneratedDraft) -> String {
    let slug = if draft.branch_slug.trim().is_empty() {
        slugify(&draft.title)
    } else {
        slugify(&draft.branch_slug)
    };
    let type_prefix = format!("{}-", draft.pr_type);
    let slug = slug.strip_prefix(&type_prefix).unwrap_or(&slug).to_string();
    if slug.is_empty() {
        String::new()
    } else {
        format!("pr/{}/{}", draft.pr_type, slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_from_draft_uses_standard_pr_prefix() {
        let draft = GeneratedDraft {
            pr_type: "fix".into(),
            branch_slug: "send-pr-range-and-form-context".into(),
            title: "Send PR range and form context".into(),
            description: "Body".into(),
        };

        assert_eq!(
            branch_from_draft(&draft),
            "pr/fix/send-pr-range-and-form-context"
        );
    }

    #[test]
    fn branch_from_draft_falls_back_to_title_slug() {
        let draft = GeneratedDraft {
            pr_type: "chore".into(),
            branch_slug: String::new(),
            title: "Clean up prompt shape".into(),
            description: "Body".into(),
        };

        assert_eq!(branch_from_draft(&draft), "pr/chore/clean-up-prompt-shape");
    }
}
