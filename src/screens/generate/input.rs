use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::domain::StatusStore;
use crate::domain::{BulkPhase, JjOpKind};
use crate::screens::{NewScreen, Transition};

use super::form::{FieldKind, FieldState, InputMode};
use super::{
    BulkItemField, BulkReviewFocus, GeneratePhase, GenerateState, JjOpDialog, Pane,
    current_revset_count, open_jj_op_dialog, update_head_from_selection,
};

pub fn on_key(state: &mut GenerateState, status: &StatusStore, key: KeyEvent) -> Transition {
    // The bulk modal captures all keys while open (mirrors the jj/picker modals).
    if !matches!(state.bulk, BulkPhase::Idle) {
        return on_bulk_modal_key(state, key);
    }
    if state.jj_op_dialog.is_some() {
        return on_jj_dialog_key(state, key);
    }
    if state.input_mode == InputMode::Editing {
        return on_editing_key(state, key);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, ctrl) {
        (KeyCode::Char('q'), false) | (KeyCode::Char('c'), true) => Transition::Quit,
        (KeyCode::Char('b'), false) => Transition::OpenBackendPicker,
        (KeyCode::Esc, _) if matches!(state.phase, GeneratePhase::Confirming { .. }) => {
            state.cancel_confirmation();
            Transition::Dirty
        }
        // While a draft is being assembled, Esc stops it (rather than leaving
        // the screen) so the user can fix the inputs and regenerate.
        (KeyCode::Esc, _)
            if matches!(
                state.phase,
                GeneratePhase::Collecting | GeneratePhase::Generating { .. }
            ) =>
        {
            Transition::CancelGeneration
        }
        (KeyCode::Esc, _) => Transition::Navigate(NewScreen::Landing),
        (KeyCode::Right, _) | (KeyCode::Char('l'), false) => {
            state.pane = state.pane.next();
            Transition::Dirty
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), false) => {
            state.pane = state.pane.prev();
            Transition::Dirty
        }
        // Generation needs only the jj range (head/base), which `start_generation`
        // guards. The form's title/branch/etc. are what the LLM fills in, so don't
        // force the user to populate them first — an empty form is fine here.
        (KeyCode::Char('g'), false) if !state.has_busy_job() && state.pane != Pane::Menu => {
            Transition::Generate
        }
        // Undo/redo of the form. `u`/`r` work on the whole form (so the LLM
        // overwriting title/description/branch is one undo); `U`/`R` step the
        // highlighted field on its own. Both live in the Form and Preview panes
        // — never the Menu, where `r` already refreshes the change list.
        (KeyCode::Char('u'), false)
            if matches!(state.pane, Pane::Form | Pane::Preview) && !state.has_busy_job() =>
        {
            let changed = state.form.undo();
            undo_redo(state, changed, "undid form change")
        }
        (KeyCode::Char('r'), false)
            if matches!(state.pane, Pane::Form | Pane::Preview) && !state.has_busy_job() =>
        {
            let changed = state.form.redo();
            undo_redo(state, changed, "redid form change")
        }
        (KeyCode::Char('U'), false) if state.pane == Pane::Form && !state.has_busy_job() => {
            let changed = state.form.undo_field(state.field_focus);
            undo_redo(state, changed, "undid field")
        }
        (KeyCode::Char('R'), false) if state.pane == Pane::Form && !state.has_busy_job() => {
            let changed = state.form.redo_field(state.field_focus);
            undo_redo(state, changed, "redid field")
        }
        (KeyCode::Char('x'), false)
            if matches!(state.phase, GeneratePhase::DraftReady { .. })
                && state.pane == Pane::Preview =>
        {
            if state.form.validate() {
                Transition::ReviewExecution
            } else {
                Transition::Dirty
            }
        }
        (KeyCode::Char('x'), false) | (KeyCode::Enter, _)
            if matches!(state.phase, GeneratePhase::Confirming { .. })
                && state.pane == Pane::Preview =>
        {
            Transition::Execute
        }
        (KeyCode::Char('c'), false) if state.done_url().is_some() => Transition::CopyUrl,
        (KeyCode::Char('o'), false) if state.done_url().is_some() => Transition::OpenUrl,
        // Menu pane navigation
        (KeyCode::Enter, _) if state.pane == Pane::Menu => {
            state.pane = Pane::Form;
            Transition::Dirty
        }
        (KeyCode::Char('r'), false) if state.pane == Pane::Menu => Transition::RefreshRevsets,
        // `G` (capital) opens the stacked-PR review modal when >= 1 head is
        // selected and no job is running.
        (KeyCode::Char('G'), false)
            if state.pane == Pane::Menu
                && !state.has_busy_job()
                && !state.selected_heads.is_empty() =>
        {
            Transition::GenerateStack
        }
        (KeyCode::Char(' '), false) if state.pane == Pane::Menu && !state.has_busy_job() => {
            if let Some(crate::domain::Revsets::Loaded(items)) = status.revsets.value()
                && let Some(item) = items.get(state.revset_selected)
            {
                let change_id = item.change_id.clone();
                state.toggle_selected_head(&change_id);
                return Transition::Dirty;
            }
            Transition::None
        }
        (KeyCode::Char('s'), false) if state.pane == Pane::Menu && !state.has_busy_job() => {
            open_jj_op_dialog(state, status, JjOpKind::SquashWithBelow)
        }
        (KeyCode::Char('J'), false) | (KeyCode::Up, true) | (KeyCode::Char('k'), true)
            if state.pane == Pane::Menu && !state.has_busy_job() =>
        {
            open_jj_op_dialog(state, status, JjOpKind::MoveUp)
        }
        (KeyCode::Char('K'), false) | (KeyCode::Down, true) | (KeyCode::Char('j'), true)
            if state.pane == Pane::Menu && !state.has_busy_job() =>
        {
            open_jj_op_dialog(state, status, JjOpKind::MoveDown)
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), false)
            if state.pane == Pane::Menu && state.revset_selected > 0 =>
        {
            state.revset_selected -= 1;
            update_head_from_selection(state, status);
            Transition::Dirty
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), false) if state.pane == Pane::Menu => {
            let n = current_revset_count(status);
            if state.revset_selected + 1 < n {
                state.revset_selected += 1;
                update_head_from_selection(state, status);
                Transition::Dirty
            } else {
                Transition::None
            }
        }
        // Form pane navigation
        (KeyCode::Up, _) | (KeyCode::Char('k'), false)
            if state.pane == Pane::Form && state.field_focus != state.field_focus.prev() =>
        {
            state.field_focus = state.field_focus.prev();
            Transition::Dirty
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), false)
            if state.pane == Pane::Form && state.field_focus != state.field_focus.next() =>
        {
            state.field_focus = state.field_focus.next();
            Transition::Dirty
        }
        (KeyCode::Char('i'), false) | (KeyCode::Enter, _) if state.pane == Pane::Form => {
            // In bulk mode the `head` field is derived/read-only: refuse editing.
            if !state.selected_heads.is_empty() && state.field_focus == super::FieldId::Head {
                return Transition::None;
            }
            state.form.field_mut(state.field_focus).begin_edit();
            state.input_mode = InputMode::Editing;
            Transition::Dirty
        }
        // Preview pane scroll
        (KeyCode::Up, _) | (KeyCode::Char('k'), false) if state.pane == Pane::Preview => {
            state.scroll_preview = state.scroll_preview.saturating_sub(1);
            Transition::Dirty
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), false) if state.pane == Pane::Preview => {
            state.scroll_preview = state.scroll_preview.saturating_add(1);
            Transition::Dirty
        }
        _ => Transition::None,
    }
}

/// Handle keys while the bulk modal is open. This captures all keys so the
/// underlying panes never see them.
fn on_bulk_modal_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    match &state.bulk {
        BulkPhase::Collecting | BulkPhase::Generating { .. } => {
            // Only Esc is meaningful: cancel the in-flight generation.
            if key.code == KeyCode::Esc {
                return Transition::CancelStack;
            }
            Transition::None
        }
        BulkPhase::Failed { .. } => {
            // Esc or Enter closes the failed modal.
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                state.bulk = BulkPhase::Idle;
                return Transition::Dirty;
            }
            Transition::None
        }
        BulkPhase::Review { .. } => on_bulk_review_key(state, key),
        BulkPhase::Idle => Transition::None,
    }
}

fn on_bulk_review_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    // While a push is in flight the modal stays responsive for *navigation* so
    // the user can keep inspecting the stack, but every mutating or
    // modal-closing action is disabled — closing or re-pushing now would
    // conflict with the running job.
    if matches!(
        state.bulk,
        BulkPhase::Review {
            pushing: Some(_),
            ..
        }
    ) {
        return match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if state.bulk_review_focus == BulkReviewFocus::Preview {
                    move_bulk_field_focus(state, -1);
                } else {
                    move_bulk_cursor(state, -1);
                }
                Transition::Dirty
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.bulk_review_focus == BulkReviewFocus::Preview {
                    move_bulk_field_focus(state, 1);
                } else {
                    move_bulk_cursor(state, 1);
                }
                Transition::Dirty
            }
            _ => Transition::None,
        };
    }
    // If currently editing a per-PR field, route to the editor.
    if state.bulk_editor.editing {
        return on_bulk_editor_key(state, key);
    }

    match key.code {
        KeyCode::Esc => {
            if state.bulk_review_focus == BulkReviewFocus::Preview {
                state.bulk_review_focus = BulkReviewFocus::List;
            } else {
                // Flush edits to plan then close.
                state.flush_bulk_editor_to_plan();
                state.bulk = BulkPhase::Idle;
            }
            Transition::Dirty
        }
        KeyCode::Char('p') => {
            state.flush_bulk_editor_to_plan();
            let index = match &state.bulk {
                BulkPhase::Review { cursor, .. } => *cursor,
                _ => return Transition::None,
            };
            Transition::PushStackPr(index)
        }
        KeyCode::Char('P') => {
            state.flush_bulk_editor_to_plan();
            Transition::PushStackAll
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.bulk_review_focus == BulkReviewFocus::Preview {
                move_bulk_field_focus(state, -1);
            } else {
                move_bulk_cursor(state, -1);
            }
            Transition::Dirty
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.bulk_review_focus == BulkReviewFocus::Preview {
                move_bulk_field_focus(state, 1);
            } else {
                move_bulk_cursor(state, 1);
            }
            Transition::Dirty
        }
        KeyCode::Enter if state.bulk_review_focus == BulkReviewFocus::List => {
            state.flush_bulk_editor_to_plan();
            state.seed_bulk_editor_from_cursor();
            state.bulk_review_focus = BulkReviewFocus::Preview;
            Transition::Dirty
        }
        KeyCode::Right => {
            if state.bulk_review_focus == BulkReviewFocus::List {
                state.flush_bulk_editor_to_plan();
                state.seed_bulk_editor_from_cursor();
                state.bulk_review_focus = BulkReviewFocus::Preview;
                Transition::Dirty
            } else {
                // Already in Preview — no-op.
                Transition::None
            }
        }
        KeyCode::Left => {
            if state.bulk_review_focus == BulkReviewFocus::Preview {
                state.bulk_review_focus = BulkReviewFocus::List;
                Transition::Dirty
            } else {
                // Already in List — do NOT close the modal.
                Transition::None
            }
        }
        KeyCode::Char('i') | KeyCode::Enter
            if state.bulk_review_focus == BulkReviewFocus::Preview =>
        {
            // Begin editing the focused per-PR field.
            state.bulk_editor.editing = true;
            let f = state.bulk_editor.field_focus;
            state.bulk_editor.field_mut(f).begin_edit();
            Transition::Dirty
        }
        KeyCode::Tab => {
            // Move field focus within the per-PR form.
            if state.bulk_review_focus == BulkReviewFocus::Preview {
                let next = state.bulk_editor.field_focus.next();
                state.bulk_editor.field_focus = next;
            }
            Transition::Dirty
        }
        KeyCode::BackTab => {
            if state.bulk_review_focus == BulkReviewFocus::Preview {
                let prev = state.bulk_editor.field_focus.prev();
                state.bulk_editor.field_focus = prev;
            }
            Transition::Dirty
        }
        _ => Transition::None,
    }
}

fn on_bulk_editor_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    let f = state.bulk_editor.field_focus;
    let multiline = f == BulkItemField::Description;
    let commit = text_commit_key(multiline, key);
    let cancel = key.code == KeyCode::Esc;

    if cancel {
        state.bulk_editor.field_mut(f).cancel();
        state.bulk_editor.editing = false;
    } else if commit {
        state.bulk_editor.field_mut(f).commit();
        state.bulk_editor.editing = false;
        // Write the edit back into the plan so the list row updates.
        state.flush_bulk_editor_to_plan();
    } else {
        state.bulk_editor.field_mut(f).input(key);
    }
    Transition::Dirty
}

fn move_bulk_field_focus(state: &mut GenerateState, delta: isize) {
    state.bulk_editor.field_focus = if delta < 0 {
        state.bulk_editor.field_focus.prev()
    } else {
        state.bulk_editor.field_focus.next()
    };
}

/// Move the bulk review cursor by `delta`. Flushes current edits to the plan,
/// moves the cursor, then re-seeds the editor from the new item.
fn move_bulk_cursor(state: &mut GenerateState, delta: isize) {
    // Flush any in-progress edits.
    if state.bulk_editor.editing {
        let f = state.bulk_editor.field_focus;
        state.bulk_editor.field_mut(f).commit();
        state.bulk_editor.editing = false;
    }
    state.flush_bulk_editor_to_plan();

    if let BulkPhase::Review { plan, cursor, .. } = &mut state.bulk {
        let n = plan.items.len();
        if n == 0 {
            return;
        }
        let next = (*cursor as isize + delta).clamp(0, (n as isize) - 1) as usize;
        *cursor = next;
    }
    state.seed_bulk_editor_from_cursor();
}

fn on_jj_dialog_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    match (state.jj_op_dialog.clone(), key.code) {
        (Some(JjOpDialog::Confirm(pending)), KeyCode::Enter) => Transition::JjOp(pending.op),
        (Some(JjOpDialog::Confirm(_)), KeyCode::Esc) => {
            state.jj_op_dialog = None;
            Transition::Dirty
        }
        (Some(JjOpDialog::Error { .. }), KeyCode::Enter | KeyCode::Esc) => {
            state.jj_op_dialog = None;
            Transition::Dirty
        }
        _ => Transition::None,
    }
}

/// Translate an undo/redo outcome into a transition, surfacing a one-line hint
/// in the Preview pane when something actually changed.
fn undo_redo(state: &mut GenerateState, changed: bool, hint: &'static str) -> Transition {
    if changed {
        state.last_action = Some(hint.to_string());
        Transition::Dirty
    } else {
        Transition::None
    }
}

fn on_editing_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    // Dispatch on the field kind (a `Copy` value) rather than borrowing the
    // field, so the commit path can route through `form.edit` — which needs to
    // re-borrow the form to record the change for undo.
    let id = state.field_focus;
    match state.form.field(id).kind() {
        FieldKind::Text { multiline } => {
            let cancel = key.code == KeyCode::Esc;
            let commit = text_commit_key(multiline, key);
            if cancel {
                state.form.field_mut(id).cancel();
                state.input_mode = InputMode::Normal;
            } else if commit {
                state.form.edit(|form| form.field_mut(id).commit());
                state.input_mode = InputMode::Normal;
            } else if let FieldState::Text(text) = state.form.field_mut(id) {
                text.input(key);
            }
            Transition::Dirty
        }
        FieldKind::Picker { multi_select, .. } => {
            match key.code {
                KeyCode::Esc => {
                    state.form.field_mut(id).cancel();
                    state.input_mode = InputMode::Normal;
                }
                KeyCode::Enter => {
                    state.form.edit(|form| form.field_mut(id).commit());
                    state.input_mode = InputMode::Normal;
                }
                _ if let FieldState::Picker(picker) = state.form.field_mut(id) => match key.code {
                    KeyCode::Char(' ') if multi_select => picker.toggle_highlighted(),
                    KeyCode::Up => picker.move_highlight(-1),
                    KeyCode::Down => picker.move_highlight(1),
                    KeyCode::Char('k') if key.modifiers.is_empty() => picker.move_highlight(-1),
                    KeyCode::Char('j') if key.modifiers.is_empty() => picker.move_highlight(1),
                    KeyCode::Char(_) | KeyCode::Backspace => picker.input_filter(key),
                    _ => {}
                },
                _ => {}
            }
            Transition::Dirty
        }
    }
}

fn text_commit_key(multiline: bool, key: KeyEvent) -> bool {
    if multiline {
        (key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::ALT))
    } else {
        key.code == KeyCode::Enter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GeneratedDraft, PromptBuild, PromptManifest, RevsetSummary, Revsets};
    use crate::screens::generate::{JjOpDialog, PrForm};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn alt_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    fn revset(change_id: &str, description: &str) -> RevsetSummary {
        RevsetSummary {
            label: format!("trunk()..{change_id}"),
            change_id: change_id.into(),
            commit_id: format!("{change_id}commit"),
            bookmarks: Vec::new(),
            description: description.into(),
            description_body: String::new(),
            author: String::new(),
            stats: String::new(),
            commit_count: 1,
            commit_ids: vec![format!("{change_id}commit")],
            change_ids: vec![change_id.into()],
            recent_log: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn status_with_two_revsets() -> StatusStore {
        let mut status = StatusStore::new();
        // Newest-first, matching jj log and the Changes pane.
        status.set_revsets(Revsets::Loaded(vec![
            revset("new", "newer change"),
            revset("old", "older change"),
        ]));
        status
    }

    fn draft_state() -> GenerateState {
        let mut form = PrForm::new("main".into());
        form.head.set_value("abcd".into());
        form.branch_name.set_value("add-foo".into());
        form.title.set_value("Add foo".into());
        form.description.set_value("Body".into());
        GenerateState {
            pane: Pane::Preview,
            revset_selected: 0,
            scroll_menu: std::cell::Cell::new(0),
            scroll_form: std::cell::Cell::new(0),
            scroll_preview: 0,
            input_mode: InputMode::Normal,
            field_focus: crate::screens::generate::FieldId::Head,
            form,
            phase: GeneratePhase::DraftReady {
                draft: GeneratedDraft {
                    pr_type: "feat".into(),
                    branch_slug: "add-foo".into(),
                    title: "Add foo".into(),
                    description: "Body".into(),
                },
                prompt: PromptBuild {
                    prompt: "prompt".into(),
                    manifest: PromptManifest {
                        sections: Vec::new(),
                        total_bytes: 0,
                    },
                },
            },
            jj_op_dialog: None,
            last_action: None,
            selected_heads: Vec::new(),
            bulk: crate::domain::BulkPhase::Idle,
            bulk_review_focus: crate::screens::generate::BulkReviewFocus::List,
            bulk_editor: crate::screens::generate::BulkItemEditor::default(),
            bulk_list_scroll: std::cell::Cell::new(0),
            bulk_form_scroll: std::cell::Cell::new(0),
        }
    }

    #[test]
    fn x_on_draft_requests_execution_review() {
        let mut state = draft_state();
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));

        assert!(matches!(transition, Transition::ReviewExecution));
        assert!(matches!(state.phase, GeneratePhase::DraftReady { .. }));
    }

    #[test]
    fn esc_from_confirmation_returns_to_draft() {
        let mut state = draft_state();
        state.begin_confirmation(&sample_forge());
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Esc));

        assert!(matches!(transition, Transition::Dirty));
        assert!(matches!(state.phase, GeneratePhase::DraftReady { .. }));
    }

    #[test]
    fn enter_from_confirmation_requests_execute() {
        let mut state = draft_state();
        state.begin_confirmation(&sample_forge());
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));

        assert!(matches!(transition, Transition::Execute));
    }

    fn sample_forge() -> crate::domain::ForgeCli {
        crate::domain::ForgeCli::new(crate::domain::ForgeKind::Gitea, "tea".into(), None)
    }

    #[test]
    fn u_undoes_the_whole_form_from_the_preview_pane() {
        let mut state = draft_state();
        state.pane = Pane::Preview;
        state.form.edit(|f| f.title.set_value("overwritten".into()));

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('u')));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.form.title(), "Add foo");
    }

    #[test]
    fn shift_u_undoes_only_the_highlighted_field() {
        use crate::screens::generate::FieldId;
        let mut state = draft_state();
        state.pane = Pane::Form;
        state.field_focus = FieldId::Title;
        state.form.edit(|f| f.title.set_value("overwritten".into()));

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('U')));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.form.title(), "Add foo");
    }

    #[test]
    fn u_with_no_history_is_inert() {
        let mut state = draft_state();
        state.pane = Pane::Preview;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('u')));

        assert!(matches!(transition, Transition::None));
    }

    #[test]
    fn esc_while_collecting_requests_cancel() {
        let mut state = draft_state();
        state.phase = GeneratePhase::Collecting;
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Esc));

        assert!(matches!(transition, Transition::CancelGeneration));
    }

    #[test]
    fn esc_when_idle_navigates_back() {
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Esc));

        assert!(matches!(
            transition,
            Transition::Navigate(NewScreen::Landing)
        ));
    }

    #[test]
    fn esc_cancels_single_line_form_edit() {
        use crate::screens::generate::FieldId;
        let mut state = draft_state();
        state.pane = Pane::Form;
        state.field_focus = FieldId::Title;
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Esc));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.form.title(), "Add foo");
    }

    #[test]
    fn enter_commits_single_line_form_edit() {
        use crate::screens::generate::FieldId;
        let mut state = draft_state();
        state.pane = Pane::Form;
        state.field_focus = FieldId::Title;
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.form.title(), "Add foox");
    }

    #[test]
    fn ctrl_s_commits_multiline_form_edit() {
        use crate::screens::generate::FieldId;
        let mut state = draft_state();
        state.pane = Pane::Form;
        state.field_focus = FieldId::Description;
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));

        let transition = on_key(
            &mut state,
            &StatusStore::new(),
            ctrl_key(KeyCode::Char('s')),
        );

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.form.description(), "Body\nx");
    }

    #[test]
    fn alt_enter_commits_multiline_form_edit() {
        use crate::screens::generate::FieldId;
        let mut state = draft_state();
        state.pane = Pane::Form;
        state.field_focus = FieldId::Description;
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));

        let transition = on_key(&mut state, &StatusStore::new(), alt_key(KeyCode::Enter));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.form.description(), "Bodyx");
    }

    #[test]
    fn shift_j_opens_move_up_dialog_against_visual_above_row() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        state.pane = Pane::Menu;
        state.revset_selected = 1;

        let transition = on_key(&mut state, &status, key(KeyCode::Char('J')));

        assert!(matches!(transition, Transition::Dirty));
        let Some(JjOpDialog::Confirm(pending)) = state.jj_op_dialog else {
            panic!("expected jj confirmation");
        };
        assert_eq!(pending.op.kind, JjOpKind::MoveUp);
        assert_eq!(pending.op.change_id, "old");
        assert_eq!(pending.op.target_id, "new");
    }

    #[test]
    fn shift_k_opens_move_down_dialog_against_visual_below_row() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        state.pane = Pane::Menu;

        let transition = on_key(&mut state, &status, key(KeyCode::Char('K')));

        assert!(matches!(transition, Transition::Dirty));
        let Some(JjOpDialog::Confirm(pending)) = state.jj_op_dialog else {
            panic!("expected jj confirmation");
        };
        assert_eq!(pending.op.kind, JjOpKind::MoveDown);
        assert_eq!(pending.op.change_id, "new");
        assert_eq!(pending.op.target_id, "old");
    }

    #[test]
    fn ctrl_up_opens_move_up_dialog() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        state.pane = Pane::Menu;
        state.revset_selected = 1;

        let transition = on_key(&mut state, &status, ctrl_key(KeyCode::Up));

        assert!(matches!(transition, Transition::Dirty));
        assert!(matches!(
            state.jj_op_dialog,
            Some(JjOpDialog::Confirm(ref pending)) if pending.op.kind == JjOpKind::MoveUp
        ));
    }

    #[test]
    fn squash_at_last_row_shows_error_dialog() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        state.pane = Pane::Menu;
        state.revset_selected = 1;

        let transition = on_key(&mut state, &status, key(KeyCode::Char('s')));

        assert!(matches!(transition, Transition::Dirty));
        assert!(matches!(state.jj_op_dialog, Some(JjOpDialog::Error { .. })));
    }

    #[test]
    fn enter_on_jj_confirm_requests_jj_transition() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        state.pane = Pane::Menu;
        let _ = on_key(&mut state, &status, key(KeyCode::Char('K')));

        let transition = on_key(&mut state, &status, key(KeyCode::Enter));

        assert!(matches!(
            transition,
            Transition::JjOp(ref op)
                if op.kind == JjOpKind::MoveDown && op.change_id == "new" && op.target_id == "old"
        ));
    }

    #[test]
    fn space_toggles_selected_head_in_changes_pane() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Idle;
        state.pane = Pane::Menu;
        state.revset_selected = 0;

        // First press: select "new"
        let transition = on_key(&mut state, &status, key(KeyCode::Char(' ')));
        assert!(matches!(transition, Transition::Dirty));
        assert!(state.selected_heads.contains(&"new".to_string()));

        // Second press: deselect "new"
        let transition = on_key(&mut state, &status, key(KeyCode::Char(' ')));
        assert!(matches!(transition, Transition::Dirty));
        assert!(!state.selected_heads.contains(&"new".to_string()));
    }

    #[test]
    fn space_is_blocked_while_job_is_in_progress() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::Collecting;
        state.pane = Pane::Menu;

        let transition = on_key(&mut state, &status, key(KeyCode::Char(' ')));
        assert!(matches!(transition, Transition::None));
        assert!(state.selected_heads.is_empty());
    }

    #[test]
    fn jj_ops_are_blocked_while_jj_mutation_is_running() {
        let status = status_with_two_revsets();
        let mut state = draft_state();
        state.phase = GeneratePhase::JjMutating {
            op: JjOpKind::MoveDown,
            summary: "moving new below old".into(),
        };
        state.pane = Pane::Menu;

        let transition = on_key(&mut state, &status, key(KeyCode::Char('s')));

        assert!(matches!(transition, Transition::None));
        assert!(state.jj_op_dialog.is_none());
    }

    #[test]
    fn p_requests_push_for_the_current_review_row() {
        let mut state = draft_state();
        state.bulk = BulkPhase::Review {
            plan: crate::domain::StackPlan {
                items: vec![
                    crate::domain::StackPlanItem {
                        input: crate::domain::StackPrInput {
                            index: 0,
                            base: "main".into(),
                            head: "a".into(),
                            included_change_ids: vec!["a".into()],
                            subject: "A".into(),
                        },
                        bookmark: "pr/feat/a".into(),
                        title: "A".into(),
                        description: "Body".into(),
                        status: crate::domain::PrStatus::Created {
                            url: "https://example.com/1".into(),
                        },
                        warnings: Vec::new(),
                        blockers: Vec::new(),
                    },
                    crate::domain::StackPlanItem {
                        input: crate::domain::StackPrInput {
                            index: 1,
                            base: "pr/feat/a".into(),
                            head: "b".into(),
                            included_change_ids: vec!["b".into()],
                            subject: "B".into(),
                        },
                        bookmark: "pr/feat/b".into(),
                        title: "B".into(),
                        description: "Body".into(),
                        status: crate::domain::PrStatus::Pending,
                        warnings: Vec::new(),
                        blockers: Vec::new(),
                    },
                ],
                labels: Vec::new(),
                assignees: Vec::new(),
                milestone: String::new(),
                intent: crate::domain::StackIntent {
                    title: String::new(),
                    description: String::new(),
                    branch: String::new(),
                },
            },
            cursor: 1,
            pushing: None,
            push_all: false,
        };

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('p')));
        assert!(matches!(transition, Transition::PushStackPr(1)));
    }

    #[test]
    fn enter_from_bulk_review_list_focuses_preview_without_editing() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::List;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::Preview);
        assert!(!state.bulk_editor.editing);
    }

    #[test]
    fn second_enter_from_bulk_review_preview_starts_field_editing() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::List;

        let first = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));
        let second = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));

        assert!(matches!(first, Transition::Dirty));
        assert!(matches!(second, Transition::Dirty));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::Preview);
        assert!(state.bulk_editor.editing);
    }

    #[test]
    fn esc_from_bulk_review_preview_returns_to_list() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::Preview;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Esc));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::List);
        assert!(matches!(state.bulk, BulkPhase::Review { .. }));
    }

    #[test]
    fn shift_p_requests_push_all() {
        let mut state = draft_state();
        state.bulk = BulkPhase::Review {
            plan: crate::domain::StackPlan {
                items: Vec::new(),
                labels: Vec::new(),
                assignees: Vec::new(),
                milestone: String::new(),
                intent: crate::domain::StackIntent {
                    title: String::new(),
                    description: String::new(),
                    branch: String::new(),
                },
            },
            cursor: 0,
            pushing: None,
            push_all: false,
        };

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('P')));
        assert!(matches!(transition, Transition::PushStackAll));
    }

    #[test]
    fn bulk_review_mutation_is_ignored_while_push_is_running() {
        let mut state = draft_state();
        state.bulk = BulkPhase::Review {
            plan: crate::domain::StackPlan {
                items: Vec::new(),
                labels: Vec::new(),
                assignees: Vec::new(),
                milestone: String::new(),
                intent: crate::domain::StackIntent {
                    title: String::new(),
                    description: String::new(),
                    branch: String::new(),
                },
            },
            cursor: 0,
            pushing: Some(0),
            push_all: true,
        };

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('p')));
        assert!(matches!(transition, Transition::None));
    }

    /// Build a two-item review plan for push-time navigation tests.
    fn two_item_review(pushing: Option<usize>) -> GenerateState {
        use crate::domain::{PrStatus, StackIntent, StackPlan, StackPlanItem, StackPrInput};
        let item = |index: usize| StackPlanItem {
            input: StackPrInput {
                index,
                base: if index == 0 {
                    "main".into()
                } else {
                    "pr/feat/0".into()
                },
                head: format!("h{index}"),
                included_change_ids: vec![format!("h{index}")],
                subject: format!("S{index}"),
            },
            bookmark: format!("pr/feat/{index}"),
            title: format!("T{index}"),
            description: "Body".into(),
            status: PrStatus::Pending,
            warnings: Vec::new(),
            blockers: Vec::new(),
        };
        let mut state = draft_state();
        state.bulk = BulkPhase::Review {
            plan: StackPlan {
                items: vec![item(0), item(1)],
                labels: Vec::new(),
                assignees: Vec::new(),
                milestone: String::new(),
                intent: StackIntent {
                    title: String::new(),
                    description: String::new(),
                    branch: String::new(),
                },
            },
            cursor: 0,
            pushing,
            push_all: pushing.is_some(),
        };
        state.seed_bulk_editor_from_cursor();
        state
    }

    #[test]
    fn bulk_review_navigation_stays_live_while_push_is_running() {
        let mut state = two_item_review(Some(0));

        // `j` moves the list cursor even though PR 0's push is in flight, and the
        // in-flight push is left untouched.
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('j')));
        assert!(matches!(transition, Transition::Dirty));
        let BulkPhase::Review {
            cursor, pushing, ..
        } = &state.bulk
        else {
            panic!("expected review phase");
        };
        assert_eq!(*cursor, 1, "navigation must move the cursor during a push");
        assert_eq!(
            *pushing,
            Some(0),
            "navigation must not disturb the in-flight push"
        );
    }

    #[test]
    fn bulk_review_mutating_and_closing_keys_are_disabled_during_push() {
        for code in [
            KeyCode::Char('p'),
            KeyCode::Char('P'),
            KeyCode::Char('i'),
            KeyCode::Enter,
            KeyCode::Esc,
        ] {
            let mut state = two_item_review(Some(0));
            let transition = on_key(&mut state, &StatusStore::new(), key(code));
            assert!(
                matches!(transition, Transition::None),
                "{code:?} must be ignored while a push is running"
            );
            // The modal stays open with the push still active.
            assert!(matches!(
                state.bulk,
                BulkPhase::Review {
                    pushing: Some(0),
                    ..
                }
            ));
            assert!(
                !state.bulk_editor.editing,
                "{code:?} must not start editing"
            );
        }
    }

    #[test]
    fn right_from_bulk_review_list_focuses_preview_without_editing() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::List;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Right));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::Preview);
        assert!(!state.bulk_editor.editing);
    }

    #[test]
    fn left_from_bulk_review_preview_returns_to_list() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::Preview;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Left));

        assert!(matches!(transition, Transition::Dirty));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::List);
        assert!(matches!(state.bulk, BulkPhase::Review { .. }));
    }

    #[test]
    fn right_while_already_preview_is_noop() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::Preview;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Right));

        assert!(matches!(transition, Transition::None));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::Preview);
    }

    #[test]
    fn left_while_already_list_does_not_close_modal() {
        let mut state = two_item_review(None);
        state.bulk_review_focus = BulkReviewFocus::List;

        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Left));

        assert!(matches!(transition, Transition::None));
        assert_eq!(state.bulk_review_focus, BulkReviewFocus::List);
        // Modal must remain open.
        assert!(matches!(state.bulk, BulkPhase::Review { .. }));
    }

    #[test]
    fn left_right_do_not_switch_panes_during_push() {
        for code in [KeyCode::Left, KeyCode::Right] {
            let mut state = two_item_review(Some(0));
            let initial_focus = state.bulk_review_focus;

            let transition = on_key(&mut state, &StatusStore::new(), key(code));

            assert!(
                matches!(transition, Transition::None),
                "{code:?} must be ignored while a push is running"
            );
            assert_eq!(
                state.bulk_review_focus, initial_focus,
                "{code:?} must not switch panes during push"
            );
        }
    }
}
