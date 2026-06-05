use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::KeyEvent;
use ratatui::Frame;

use crate::config::{Config, LlmApi};
use crate::domain::{
    BackendHealth, BackendHealthProbe, BaseBookmarks, BaseBookmarksProbe, ContextJob,
    ContextResult, ExecutePrJob, ExecuteResult, GeneratedDraft, JjMutateJob, JjMutateResult, JjOp,
    LlmGenerateJob, LlmResult, PromptForm, RepoOptions, RepoOptionsProbe, RevsetProbe, RevsetStats,
    RevsetStatsProbe, Revsets, StackContextJob, StackContextResult, StackExistingPrsProbe,
    StackLlmResult, StackPrLlmJob, StackPushJob, StackPushPrecheck, StackPushPrecheckJob,
    StackPushResult, StatusStore, TeaAuthProbe, TeaAuthStatus, VersionKind, VersionProbe,
    VersionResult, WorkspaceInfo, WorkspaceProbe, annotate_blockers, annotate_order_blockers,
    build_prompt, build_stack_prefix, derive_stack_ranges, fallback_stack_draft,
    mark_created_from_existing_prs, slugify, stack_pr_suffix,
};
use crate::domain::{
    BulkPhase, CacheHealth, StackDraft, StackIntent, StackPlan, StackPlanItem, StackPrInput,
};
use crate::input::InputEvent;
use crate::runtime::{CancelHandle, JobEvent, JobOutcomeEvent, JobSubmitter};
use crate::screens::backend_picker::{BackendPicker, PickerOutcome};
use crate::screens::generate::{GeneratePhase, PrForm};
use crate::screens::{self, GenerateState, LandingState, NewScreen, Screen, Transition};

/// Fallback byte budget for the aggregate diff when a backend doesn't set its
/// own `diff_budget_bytes`. Per-change diffs are no longer sent, so the whole
/// budget backs one diff. A backend can override this (including 0 to omit the
/// diff entirely; see `diff_budget_bytes`) to fit a smaller context window —
/// a too-large diff overflows the window and times out mid-prefill.
const CONTEXT_DIFF_BUDGET_BYTES: usize = 128 * 1024;

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
    pending_stack_push: Option<PendingStackPush>,
    /// Aborts the in-flight LLM generation request. `Some` only while a
    /// generation job is running; taken when the result lands or the user
    /// cancels. See [`App::cancel_generation`].
    gen_cancel: Option<CancelHandle>,
    quit: bool,
    dirty: bool,
}

#[derive(Debug, Clone, Copy)]
struct PendingStackPush {
    index: usize,
    push_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackGenerationAction {
    None,
    Submit(usize),
    Finish,
}

/// Downcast and dispatch one job payload in [`App::absorb_payload`].
///
/// Every job result arrives type-erased as `Box<dyn Any + Send>`, so the
/// consumer must try each known payload type in turn. Written out, each attempt
/// is the same `match any.downcast::<T>() { Ok(b) => { …; return }, Err(a) => a }`
/// scaffold. This macro collapses that to one line plus the typed body: on a
/// match it unboxes the payload, binds it as `$bind`, runs `$body`, and returns;
/// otherwise it rebinds `$any` to the still-boxed value so the next invocation
/// can try its own type.
///
/// The body keeps each arm's exact side effects — inline status updates still
/// set `self.dirty`, and delegating arms still defer to a typed handler that
/// owns its own dirty/stale-guard logic — so adding a payload type means one
/// `try_payload!` line, not a fresh downcast scaffold.
macro_rules! try_payload {
    ($any:ident, $ty:ty, $bind:ident => $body:block) => {
        let $any = match $any.downcast::<$ty>() {
            Ok(boxed) => {
                let $bind = *boxed;
                $body
                return;
            }
            Err(still_boxed) => still_boxed,
        };
    };
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
            pending_stack_push: None,
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
                self.refresh_revsets();
            }
            Transition::OpenBackendPicker => self.open_backend_picker(),
            Transition::JjOp(op) => self.start_jj_mutation(op),
            Transition::GenerateStack => self.start_stack_generation(),
            Transition::CancelStack => self.cancel_stack_generation(),
            Transition::PushStackPr(index) => self.start_stack_push(index, false),
            Transition::PushStackAll => self.start_stack_push(0, true),
        }
    }

    fn refresh_revsets(&mut self) {
        self.status.revsets.mark_loading();
        self.submitter.submit(RevsetProbe {
            jj_binary: self.config.commands.jj.clone(),
            revset: "trunk()..@".into(),
        });
        self.dirty = true;
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
        let diff_byte_budget = self
            .config
            .llm
            .active_backend()
            .diff_budget_bytes
            .unwrap_or(CONTEXT_DIFF_BUDGET_BYTES);
        self.submitter.submit(ContextJob {
            jj_binary: self.config.commands.jj.clone(),
            base: base.clone(),
            head: head.clone(),
            diff_byte_budget,
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
            state.last_action = Some("generation cancelled".to_string());
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

    fn start_jj_mutation(&mut self, op: JjOp) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        if state.is_in_progress() {
            tracing::debug!(target: "teatui::jj_mutate", "ignored: already in progress");
            return;
        }
        let Some(pending) = state.take_confirmed_jj_op(&op) else {
            tracing::debug!(target: "teatui::jj_mutate", "ignored: no matching confirmation");
            return;
        };
        let summary = pending.summary();
        state.phase = GeneratePhase::JjMutating {
            op: op.kind,
            summary,
        };
        state.last_action = None;
        self.dirty = true;
        self.submitter.submit(JjMutateJob {
            jj_binary: self.config.commands.jj.clone(),
            op,
            // Conservative: this is also the revset backing the visible Changes pane.
            conflict_revset: "trunk()..@".into(),
        });
        tracing::info!(target: "teatui::jj_mutate", "submitted");
    }

    fn start_stack_generation(&mut self) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        if state.has_busy_job() {
            tracing::debug!(target: "teatui::stack", "ignored: busy");
            return;
        }
        let Some(crate::domain::Revsets::Loaded(revsets)) = self.status.revsets.value() else {
            return;
        };
        if state.selected_heads.is_empty() {
            return;
        }
        let base = state.form.base().to_string();
        let ranges = derive_stack_ranges(revsets, &state.selected_heads, &base);
        if ranges.is_empty() {
            tracing::warn!(target: "teatui::stack", "ignored: no valid ranges derived");
            return;
        }

        // Transition to Collecting and submit the context job. The form state
        // (base, intent, labels, etc.) is stable during Collecting/Generating;
        // it is re-read when the context and LLM results land so we do not need
        // to clone it all here.
        state.bulk = BulkPhase::Collecting;
        state.last_action = None;
        self.dirty = true;

        let n_ranges = ranges.len();
        let diff_budget = self
            .config
            .llm
            .active_backend()
            .diff_budget_bytes
            .unwrap_or(CONTEXT_DIFF_BUDGET_BYTES);

        self.submitter.submit(StackContextJob {
            jj_binary: self.config.commands.jj.clone(),
            ranges,
            total_diff_byte_budget: diff_budget,
        });
        tracing::info!(target: "teatui::stack", ranges = n_ranges, "started context collection");
    }

    fn cancel_stack_generation(&mut self) {
        if let Some(cancel) = self.gen_cancel.take() {
            cancel.cancel();
        }
        if let Screen::Generate(state) = &mut self.screen {
            match &state.bulk {
                BulkPhase::Collecting | BulkPhase::Generating { .. } => {
                    state.bulk = BulkPhase::Idle;
                    state.last_action = Some("stack generation cancelled".to_string());
                    tracing::info!(target: "teatui::stack", "cancelled by user");
                }
                _ => {}
            }
        }
        self.dirty = true;
    }

    fn start_stack_push(&mut self, requested_index: usize, push_all: bool) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        if state.has_busy_job() {
            tracing::debug!(target: "teatui::stack", "ignored: busy");
            return;
        }
        state.flush_bulk_editor_to_plan();

        let BulkPhase::Review { plan, .. } = &mut state.bulk else {
            tracing::debug!(target: "teatui::stack", "ignored: not in review");
            return;
        };

        if push_all {
            let Some(index) = next_stack_push_index(plan, 0) else {
                state.last_action = Some("stack already pushed".to_string());
                self.dirty = true;
                return;
            };
            self.start_stack_push_precheck(index, true);
            return;
        }

        let Some(item) = plan.items.get(requested_index) else {
            return;
        };
        if matches!(item.status, crate::domain::PrStatus::Created { .. }) {
            state.last_action = Some("PR already created".to_string());
            self.dirty = true;
            return;
        }
        if let Some((index, _)) = plan
            .items
            .iter()
            .enumerate()
            .take(requested_index)
            .find(|(_, item)| !matches!(item.status, crate::domain::PrStatus::Created { .. }))
        {
            state.last_action = Some(format!("wait for PR {} to be created first", index + 1));
            self.dirty = true;
            return;
        }
        if let Some(reason) = item.blockers.first().cloned() {
            state.last_action = Some(reason);
            self.dirty = true;
            return;
        }

        self.start_stack_push_precheck(requested_index, false);
    }

    fn start_stack_push_precheck(&mut self, index: usize, push_all: bool) {
        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let BulkPhase::Review {
            plan,
            cursor,
            pushing,
            push_all: current_push_all,
        } = &mut state.bulk
        else {
            return;
        };
        if plan.items.get(index).is_none() {
            return;
        }
        *cursor = index;
        *pushing = Some(index);
        *current_push_all = push_all;
        state.seed_bulk_editor_from_cursor();
        state.last_action = Some("checking current PR state".to_string());
        self.pending_stack_push = Some(PendingStackPush { index, push_all });
        self.dirty = true;
        self.submitter.submit(StackPushPrecheckJob {
            jj_binary: self.config.commands.jj.clone(),
            tea_binary: self.config.commands.tea.clone(),
        });
        tracing::info!(target: "teatui::stack", index, push_all, "submitted push precheck");
    }

    fn submit_stack_push(&mut self, index: usize, push_all: bool) {
        let (item, labels, assignees, milestone) = {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            let BulkPhase::Review {
                plan,
                cursor,
                pushing,
                push_all: current_push_all,
            } = &mut state.bulk
            else {
                return;
            };

            if let Some(item) = plan.items.get(index)
                && let Some(reason) = item.blockers.first().cloned()
            {
                state.last_action = Some(reason);
                self.dirty = true;
                return;
            }

            let Some(item) = plan.items.get(index).cloned() else {
                return;
            };
            *cursor = index;
            *pushing = Some(index);
            *current_push_all = push_all;
            (
                item,
                plan.labels.clone(),
                plan.assignees.clone(),
                plan.milestone.clone(),
            )
        };

        if let Screen::Generate(state) = &mut self.screen {
            state.seed_bulk_editor_from_cursor();
            state.last_action = None;
        }
        self.dirty = true;

        self.submitter.submit(StackPushJob {
            jj_binary: self.config.commands.jj.clone(),
            tea_binary: self.config.commands.tea.clone(),
            item,
            labels,
            assignees,
            milestone,
        });
        tracing::info!(target: "teatui::stack", index, push_all, "submitted push job");
    }

    fn handle_stack_context_result(&mut self, result: StackContextResult) {
        {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            if !matches!(state.bulk, BulkPhase::Collecting) {
                tracing::debug!(target: "teatui::stack", "stale stack context result ignored");
                return;
            }
        }

        match result {
            StackContextResult::Ready { bundles, inputs } => {
                // Collect the data we need from the screen state before mutating.
                let (intent, labels, assignees, milestone) = {
                    let Screen::Generate(state) = &mut self.screen else {
                        return;
                    };
                    (
                        StackIntent {
                            title: state.form.title().to_string(),
                            description: state.form.description().to_string(),
                            branch: state.form.branch().to_string(),
                        },
                        state.form.labels(),
                        state.form.assignees(),
                        state.form.milestone().to_string(),
                    )
                };

                let prefix = build_stack_prefix(&bundles, &inputs, &intent, &labels, &milestone);
                let cancel = CancelHandle::new();
                let total = inputs.len();
                let Screen::Generate(state) = &mut self.screen else {
                    return;
                };
                state.bulk = BulkPhase::Generating {
                    prefix: Arc::from(prefix.prefix),
                    inputs,
                    intent,
                    labels,
                    assignees,
                    milestone,
                    drafts: vec![None; total],
                    warnings: vec![Vec::new(); total],
                    next: 0,
                    total,
                };
                self.gen_cancel = Some(cancel);
                self.submit_stack_pr_llm(0);
                tracing::info!(target: "teatui::stack", total, "stack context ready, submitted first llm row");
            }
            StackContextResult::Errored { index, message } => {
                let Screen::Generate(state) = &mut self.screen else {
                    return;
                };
                let msg = format!("context collection failed at PR {index}: {message}");
                tracing::error!(target: "teatui::stack", %message, "stack context failed");
                state.bulk = BulkPhase::Failed { message: msg };
            }
        }
        self.dirty = true;
    }

    fn handle_stack_llm_result(&mut self, result: StackLlmResult) {
        {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            if !matches!(state.bulk, BulkPhase::Generating { .. }) {
                tracing::debug!(target: "teatui::stack", "stale stack llm result ignored");
                return;
            }
        }

        let action = match result {
            StackLlmResult::Ready {
                index,
                draft,
                cache,
            } => self.record_stack_draft(index, draft, cache, None),
            StackLlmResult::Errored {
                index,
                message,
                fallback,
                cache,
            } => self.record_stack_draft(index, fallback, cache, Some(message)),
            StackLlmResult::Cancelled { .. } => {
                self.gen_cancel = None;
                let Screen::Generate(state) = &mut self.screen else {
                    return;
                };
                tracing::info!(target: "teatui::stack", "stack llm cancelled");
                state.bulk = BulkPhase::Idle;
                state.last_action = Some("stack generation cancelled".to_string());
                StackGenerationAction::None
            }
        };

        match action {
            StackGenerationAction::None => {}
            StackGenerationAction::Submit(index) => self.submit_stack_pr_llm(index),
            StackGenerationAction::Finish => self.finish_stack_generation(),
        }
        self.dirty = true;
    }

    fn submit_stack_pr_llm(&mut self, index: usize) {
        let Some(cancel) = self.gen_cancel.clone() else {
            return;
        };
        let Some((prefix, input, suffix)) = self.stack_pr_llm_payload(index) else {
            return;
        };
        let llm = self.config.llm.active_backend();
        self.submitter.submit(StackPrLlmJob {
            base_url: llm.base_url.clone(),
            model: llm.model.clone(),
            api: llm.api,
            api_key: llm.api_key.clone(),
            prefix,
            suffix,
            temperature: llm.temperature,
            max_tokens: llm.max_tokens,
            timeout: Duration::from_secs(llm.timeout_secs),
            cancel,
            input,
        });
        if let Screen::Generate(state) = &mut self.screen
            && let BulkPhase::Generating { next, .. } = &mut state.bulk
        {
            *next = index + 1;
        }
        tracing::info!(target: "teatui::stack", index, "submitted stack llm row");
    }

    fn stack_pr_llm_payload(&self, index: usize) -> Option<(Arc<str>, StackPrInput, String)> {
        let Screen::Generate(state) = &self.screen else {
            return None;
        };
        let BulkPhase::Generating {
            prefix,
            inputs,
            total,
            ..
        } = &state.bulk
        else {
            return None;
        };
        if index >= *total {
            return None;
        }
        let input = inputs.get(index)?.clone();
        let suffix = stack_pr_suffix(&input);
        Some((Arc::clone(prefix), input, suffix))
    }

    fn record_stack_draft(
        &mut self,
        index: usize,
        draft: StackDraft,
        cache: CacheHealth,
        error: Option<String>,
    ) -> StackGenerationAction {
        let api = self.config.llm.active_backend().api;
        let Screen::Generate(state) = &mut self.screen else {
            return StackGenerationAction::None;
        };
        let BulkPhase::Generating {
            drafts,
            warnings,
            next,
            total,
            ..
        } = &mut state.bulk
        else {
            tracing::debug!(target: "teatui::stack", "stale stack llm result ignored");
            return StackGenerationAction::None;
        };
        if index >= *total {
            tracing::debug!(target: "teatui::stack", index, total, "out-of-range stack llm result ignored");
            return StackGenerationAction::None;
        }

        if let Some(message) = error {
            warnings[index].push(format!("LLM fallback: {message}"));
        }
        if let Some(message) = stack_cache_warning(api, index, &cache) {
            warnings[index].push(message);
        }
        drafts[index] = Some(draft);

        if drafts.iter().all(Option::is_some) {
            StackGenerationAction::Finish
        } else if *next < *total {
            StackGenerationAction::Submit(*next)
        } else {
            StackGenerationAction::None
        }
    }

    fn finish_stack_generation(&mut self) {
        self.gen_cancel = None;

        let (inputs, intent, labels, assignees, milestone, drafts, warnings) = {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            let phase = std::mem::replace(&mut state.bulk, BulkPhase::Idle);
            let BulkPhase::Generating {
                inputs,
                intent,
                labels,
                assignees,
                milestone,
                drafts,
                warnings,
                ..
            } = phase
            else {
                return;
            };
            (
                inputs, intent, labels, assignees, milestone, drafts, warnings,
            )
        };

        let drafts: Vec<StackDraft> = drafts
            .into_iter()
            .enumerate()
            .map(|(index, draft)| {
                draft.unwrap_or_else(|| {
                    inputs
                        .get(index)
                        .map(fallback_stack_draft)
                        .unwrap_or_else(|| StackDraft {
                            index,
                            pr_type: "chore".into(),
                            branch_slug: String::new(),
                            title: format!("PR {}", index + 1),
                            description: String::new(),
                        })
                })
            })
            .collect();

        let items = build_plan_items(&inputs, &drafts, &warnings);
        let plan = StackPlan {
            items,
            labels,
            assignees,
            milestone,
            intent,
        };

        {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            state.bulk = BulkPhase::Review {
                plan,
                cursor: 0,
                pushing: None,
                push_all: false,
            };
            state.bulk_review_focus = crate::screens::generate::BulkReviewFocus::List;
            state.seed_bulk_editor_from_cursor();
        }
        self.refresh_stack_review_blockers();
        self.status.mark_stack_existing_prs_loading();
        self.submitter.submit(StackExistingPrsProbe {
            tea_binary: self.config.commands.tea.clone(),
        });
        tracing::info!(target: "teatui::stack", "stack llm complete, entering review");
    }

    fn handle_stack_push_precheck(&mut self, result: StackPushPrecheck) {
        self.status.set_base_bookmarks(result.bookmarks);
        self.status.set_stack_existing_prs(result.existing_prs);
        self.refresh_stack_review_blockers();

        let Some(pending) = self.pending_stack_push.take() else {
            tracing::debug!(target: "teatui::stack", "stale stack push precheck ignored");
            self.dirty = true;
            return;
        };

        let mut already_created = false;
        let mut blocker = None;
        {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            let BulkPhase::Review {
                plan,
                pushing,
                push_all,
                ..
            } = &mut state.bulk
            else {
                return;
            };
            if *pushing != Some(pending.index) {
                tracing::debug!(target: "teatui::stack", "stale stack push precheck ignored");
                return;
            }

            if let Some(item) = plan.items.get(pending.index) {
                already_created = matches!(item.status, crate::domain::PrStatus::Created { .. });
                blocker = item.blockers.first().cloned();
            }

            if already_created || blocker.is_some() {
                *pushing = None;
                if blocker.is_some() || !pending.push_all {
                    *push_all = false;
                }
            }

            if already_created {
                state.last_action = Some("PR already created".to_string());
            } else if let Some(reason) = &blocker {
                state.last_action = Some(reason.clone());
            }
        }

        if already_created && pending.push_all {
            let next = {
                let Screen::Generate(state) = &mut self.screen else {
                    return;
                };
                let BulkPhase::Review { plan, .. } = &state.bulk else {
                    return;
                };
                next_stack_push_index(plan, pending.index + 1)
            };
            if let Some(next_index) = next {
                self.start_stack_push_precheck(next_index, true);
                return;
            }
            if let Screen::Generate(state) = &mut self.screen {
                if let BulkPhase::Review {
                    pushing, push_all, ..
                } = &mut state.bulk
                {
                    *pushing = None;
                    *push_all = false;
                }
                state.last_action = Some("stack push complete".to_string());
            }
            self.dirty = true;
            return;
        }

        if blocker.is_none() && !already_created {
            self.submit_stack_push(pending.index, pending.push_all);
            return;
        }

        self.dirty = true;
    }

    fn handle_stack_push_result(&mut self, result: StackPushResult) {
        let mut continue_from: Option<usize> = None;
        {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            let BulkPhase::Review {
                plan,
                cursor,
                pushing,
                push_all,
            } = &mut state.bulk
            else {
                tracing::debug!(target: "teatui::stack", "stale stack push result ignored");
                return;
            };
            let Some(active_index) = pushing.take() else {
                tracing::debug!(target: "teatui::stack", "stale stack push result ignored");
                return;
            };
            if active_index != result.index {
                tracing::debug!(
                    target: "teatui::stack",
                    active_index,
                    result.index,
                    "stack push result did not match active item"
                );
                return;
            }

            let status = {
                let Some(item) = plan.items.get_mut(result.index) else {
                    return;
                };
                item.status = result.status.clone();
                *cursor = result.index;
                state.bulk_editor = crate::screens::generate::BulkItemEditor::from_plan_item(item);
                item.status.clone()
            };

            state.last_action = match &status {
                crate::domain::PrStatus::Created { .. } => Some("PR created".to_string()),
                crate::domain::PrStatus::Failed { step, message } => {
                    Some(format!("{}: {}", step.label(), message))
                }
                crate::domain::PrStatus::Bookmarked => Some("bookmark set".to_string()),
                crate::domain::PrStatus::Pushed => Some("bookmark pushed".to_string()),
                crate::domain::PrStatus::Pending => None,
            };

            if matches!(status, crate::domain::PrStatus::Created { .. }) && *push_all {
                continue_from = next_stack_push_index(plan, result.index + 1);
                if continue_from.is_none() {
                    state.last_action = Some("stack push complete".to_string());
                }
            }

            if !matches!(status, crate::domain::PrStatus::Created { .. }) || continue_from.is_none()
            {
                *push_all = false;
            }
        }

        self.refresh_stack_review_blockers();

        if let Some(next_index) = continue_from {
            self.start_stack_push_precheck(next_index, true);
            return;
        }

        self.dirty = true;
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
                state.last_action = Some("copied to clipboard".to_string());
                tracing::info!(target: "teatui::execute", %url, "copied URL");
            }
            Err(e) => {
                tracing::error!(target: "teatui::execute", error = %e, "clipboard write failed");
                state.last_action = Some("clipboard write failed".to_string());
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
                state.last_action = Some("opened in browser".to_string());
                tracing::info!(target: "teatui::execute", %url, "opened URL");
            }
            Err(e) => {
                tracing::error!(target: "teatui::execute", error = %e, "open failed");
                state.last_action = Some("open failed".to_string());
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
        try_payload!(any, VersionResult, version => {
            self.status.set_version(version);
            self.dirty = true;
        });
        try_payload!(any, WorkspaceInfo, workspace => {
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
        });
        try_payload!(any, TeaAuthStatus, auth => {
            self.status.set_tea_auth(auth);
            self.dirty = true;
        });
        try_payload!(any, BackendHealth, payload => {
            let BackendHealth { name, health } = payload;
            // The active backend also drives the landing LLM chip.
            if name == self.config.llm.active_backend().name {
                self.status.set_llm(health.clone());
            }
            self.status.set_backend_health(name, health);
            self.dirty = true;
        });
        try_payload!(any, Revsets, revsets => {
            self.status.set_revsets(revsets);
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
            self.refresh_stack_review_blockers();
            self.dirty = true;
        });
        try_payload!(any, RevsetStats, stats => {
            self.status.merge_revset_stats(stats);
            self.dirty = true;
        });
        try_payload!(any, BaseBookmarks, bookmarks => {
            self.status.set_base_bookmarks(bookmarks);
            self.sync_generate_options();
            self.refresh_stack_review_blockers();
            self.dirty = true;
        });
        try_payload!(any, crate::domain::StackExistingPrs, prs => {
            self.status.set_stack_existing_prs(prs);
            self.refresh_stack_review_blockers();
            self.dirty = true;
        });
        try_payload!(any, StackPushPrecheck, precheck => {
            self.handle_stack_push_precheck(precheck);
        });
        try_payload!(any, RepoOptions, options => {
            self.status.set_repo_options(options);
            self.sync_generate_options();
            self.dirty = true;
        });
        try_payload!(any, ContextResult, result => {
            self.handle_context_result(result);
        });
        try_payload!(any, LlmResult, result => {
            self.handle_llm_result(result);
        });
        try_payload!(any, ExecuteResult, result => {
            self.handle_execute_result(result);
        });
        try_payload!(any, StackPushResult, result => {
            self.handle_stack_push_result(result);
        });
        try_payload!(any, JjMutateResult, result => {
            self.handle_jj_mutate_result(result);
        });
        try_payload!(any, StackContextResult, result => {
            self.handle_stack_context_result(result);
        });
        try_payload!(any, StackLlmResult, result => {
            self.handle_stack_llm_result(result);
        });
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
                // Record the overwrite as one edit so the user can `u` back to
                // the hints they typed if the draft comes back as nonsense.
                state.form.edit(|form| {
                    form.title.set_value(draft.title.clone());
                    form.description.set_value(draft.description.clone());
                    let branch = branch_from_draft(&draft);
                    if !branch.is_empty() {
                        form.branch_name.set_value(branch);
                    }
                });
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
                state.last_action = Some("generation cancelled".to_string());
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

    fn handle_jj_mutate_result(&mut self, result: JjMutateResult) {
        let mut refresh = false;
        {
            let Screen::Generate(state) = &mut self.screen else {
                return;
            };
            if !matches!(state.phase, GeneratePhase::JjMutating { .. }) {
                tracing::debug!(target: "teatui::jj_mutate", "stale result ignored");
                return;
            }
            match result {
                JjMutateResult::Applied { op } => {
                    tracing::info!(target: "teatui::jj_mutate", op = op.label(), "applied");
                    state
                        .reset_after_jj_mutation(self.config.pr.default_base.clone(), &self.status);
                    state.last_action = Some("jj operation applied".to_string());
                    refresh = true;
                }
                JjMutateResult::Reverted { op, reason } => {
                    tracing::warn!(target: "teatui::jj_mutate", op = op.label(), %reason, "reverted");
                    state.phase = GeneratePhase::Idle;
                    state.show_jj_error(format!("{} reverted", op.label()), reason);
                    refresh = true;
                }
                JjMutateResult::Errored { op, message } => {
                    tracing::error!(target: "teatui::jj_mutate", op = op.label(), %message, "failed");
                    state.phase = GeneratePhase::Idle;
                    state.show_jj_error(format!("{} failed", op.label()), message);
                    self.dirty = true;
                }
            }
        }
        if refresh {
            self.refresh_revsets();
        }
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

    fn refresh_stack_review_blockers(&mut self) {
        let local_bookmarks = collect_stack_bookmarks(&self.status);
        let existing_prs = self
            .status
            .stack_existing_prs
            .value()
            .cloned()
            .unwrap_or_default();

        let Screen::Generate(state) = &mut self.screen else {
            return;
        };
        let BulkPhase::Review { plan, .. } = &mut state.bulk else {
            return;
        };

        mark_created_from_existing_prs(plan, &existing_prs);
        annotate_blockers(plan, &local_bookmarks, &existing_prs);
        annotate_order_blockers(plan);
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

fn collect_stack_bookmarks(status: &StatusStore) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    if let Some(crate::domain::Revsets::Loaded(items)) = status.revsets.value() {
        for item in items {
            for bookmark in &item.bookmarks {
                let bookmark = bookmark.trim();
                if !bookmark.is_empty() && seen.insert(bookmark.to_string()) {
                    out.push(bookmark.to_string());
                }
            }
        }
    }

    if let Some(bookmarks) = status.base_bookmarks.value() {
        for bookmark in bookmarks {
            let name = bookmark.name.trim();
            if !name.is_empty() && seen.insert(name.to_string()) {
                out.push(name.to_string());
            }
        }
    }

    out
}

fn stack_cache_warning(api: LlmApi, index: usize, cache: &CacheHealth) -> Option<String> {
    if index == 0 || api != LlmApi::Openai {
        return None;
    }
    let cached = cache.cached_tokens?;
    if cached == 0 {
        return Some(
            "LLM prefix cache appears cold; llama.cpp SWA may need --swa-full, vLLM needs prefix caching".into(),
        );
    }
    if let Some(prompt_tokens) = cache.prompt_tokens
        && prompt_tokens > 0
        && cached.saturating_mul(2) < prompt_tokens
    {
        return Some(format!(
            "LLM prefix cache reused only {cached}/{prompt_tokens} prompt tokens"
        ));
    }
    None
}

fn next_stack_push_index(plan: &StackPlan, start: usize) -> Option<usize> {
    plan.items
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, item)| match item.status {
            crate::domain::PrStatus::Created { .. } => None,
            _ => Some(index),
        })
}

/// Build `StackPlanItem` entries from the LLM drafts. The bookmark for each
/// item is built via the same `pr/{type}/{slug}` logic as `branch_from_draft`.
fn build_plan_items(
    ranges: &[StackPrInput],
    drafts: &[StackDraft],
    warnings: &[Vec<String>],
) -> Vec<StackPlanItem> {
    use crate::domain::PrStatus;

    // Update bases: PR k's base should be PR k-1's bookmark (not its head
    // change_id as stored in derive_stack_ranges). We build bookmarks first
    // so we can chain them.
    let bookmarks: Vec<String> = drafts
        .iter()
        .map(|d| bookmark_from_draft_fields(&d.pr_type, &d.branch_slug, &d.title))
        .collect();

    ranges
        .iter()
        .zip(drafts.iter())
        .enumerate()
        .map(|(i, (input, draft))| {
            // Update the base: PR 0 keeps the form base; PR k uses the previous
            // PR's bookmark (which may now be different from the head change_id).
            let base = if i == 0 {
                input.base.clone()
            } else {
                bookmarks[i - 1].clone()
            };
            let updated_input = StackPrInput {
                base,
                ..input.clone()
            };
            StackPlanItem {
                input: updated_input,
                bookmark: bookmarks[i].clone(),
                title: draft.title.clone(),
                description: draft.description.clone(),
                status: PrStatus::Pending,
                warnings: warnings.get(i).cloned().unwrap_or_default(),
                blockers: Vec::new(),
            }
        })
        .collect()
}

fn bookmark_from_draft_fields(pr_type: &str, branch_slug: &str, title: &str) -> String {
    let slug = if branch_slug.trim().is_empty() {
        slugify(title)
    } else {
        slugify(branch_slug)
    };
    let type_prefix = format!("{pr_type}-");
    let slug = slug.strip_prefix(&type_prefix).unwrap_or(&slug).to_string();
    if slug.is_empty() {
        String::new()
    } else {
        format!("pr/{pr_type}/{slug}")
    }
}

/// Single-PR branch name from a draft. Shares `bookmark_from_draft_fields` with
/// the stacked path so the two can never derive different bookmarks from the
/// same draft.
fn branch_from_draft(draft: &GeneratedDraft) -> String {
    bookmark_from_draft_fields(&draft.pr_type, &draft.branch_slug, &draft.title)
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
