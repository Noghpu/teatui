use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::domain::JjOpKind;
use crate::domain::StatusStore;
use crate::screens::{NewScreen, Transition};

use super::form::{FieldKind, FieldState, InputMode};
use super::{
    GeneratePhase, GenerateState, JjOpDialog, Pane, current_revset_count, open_jj_op_dialog,
    update_head_from_selection,
};

pub fn on_key(state: &mut GenerateState, status: &StatusStore, key: KeyEvent) -> Transition {
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
        (KeyCode::Char('g'), false) if !state.is_in_progress() && state.pane != Pane::Menu => {
            Transition::Generate
        }
        // Undo/redo of the form. `u`/`r` work on the whole form (so the LLM
        // overwriting title/description/branch is one undo); `U`/`R` step the
        // highlighted field on its own. Both live in the Form and Preview panes
        // — never the Menu, where `r` already refreshes the change list.
        (KeyCode::Char('u'), false)
            if matches!(state.pane, Pane::Form | Pane::Preview) && !state.is_in_progress() =>
        {
            let changed = state.form.undo();
            undo_redo(state, changed, "undid form change")
        }
        (KeyCode::Char('r'), false)
            if matches!(state.pane, Pane::Form | Pane::Preview) && !state.is_in_progress() =>
        {
            let changed = state.form.redo();
            undo_redo(state, changed, "redid form change")
        }
        (KeyCode::Char('U'), false) if state.pane == Pane::Form && !state.is_in_progress() => {
            let changed = state.form.undo_field(state.field_focus);
            undo_redo(state, changed, "undid field")
        }
        (KeyCode::Char('R'), false) if state.pane == Pane::Form && !state.is_in_progress() => {
            let changed = state.form.redo_field(state.field_focus);
            undo_redo(state, changed, "redid field")
        }
        (KeyCode::Char('x'), false)
            if matches!(state.phase, GeneratePhase::DraftReady { .. })
                && state.pane == Pane::Preview =>
        {
            if state.form.validate() {
                state.begin_confirmation();
                Transition::Dirty
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
        (KeyCode::Char('s'), false) if state.pane == Pane::Menu && !state.is_in_progress() => {
            open_jj_op_dialog(state, status, JjOpKind::SquashWithBelow)
        }
        (KeyCode::Char('J'), false) | (KeyCode::Up, true) | (KeyCode::Char('k'), true)
            if state.pane == Pane::Menu && !state.is_in_progress() =>
        {
            open_jj_op_dialog(state, status, JjOpKind::MoveUp)
        }
        (KeyCode::Char('K'), false) | (KeyCode::Down, true) | (KeyCode::Char('j'), true)
            if state.pane == Pane::Menu && !state.is_in_progress() =>
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
        state.last_action = Some(hint);
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
            // Esc commits and exits — there is no separate "cancel" for
            // textareas; if the user wants to discard a change they can
            // re-enter and overwrite. Enter commits a single-line field;
            // in multiline it inserts a newline as expected.
            let commit = key.code == KeyCode::Esc || (!multiline && key.code == KeyCode::Enter);
            if commit {
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
        }
    }

    #[test]
    fn x_on_draft_enters_confirmation_instead_of_executing() {
        let mut state = draft_state();
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));

        assert!(matches!(transition, Transition::Dirty));
        assert!(matches!(state.phase, GeneratePhase::Confirming { .. }));
    }

    #[test]
    fn esc_from_confirmation_returns_to_draft() {
        let mut state = draft_state();
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Esc));

        assert!(matches!(transition, Transition::Dirty));
        assert!(matches!(state.phase, GeneratePhase::DraftReady { .. }));
    }

    #[test]
    fn enter_from_confirmation_requests_execute() {
        let mut state = draft_state();
        let _ = on_key(&mut state, &StatusStore::new(), key(KeyCode::Char('x')));
        let transition = on_key(&mut state, &StatusStore::new(), key(KeyCode::Enter));

        assert!(matches!(transition, Transition::Execute));
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
}
