use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::domain::StatusStore;
use crate::screens::{NewScreen, Transition};

use super::form::{FieldState, InputMode};
use super::{GeneratePhase, GenerateState, Pane, current_revset_count, update_head_from_selection};

pub fn on_key(state: &mut GenerateState, status: &StatusStore, key: KeyEvent) -> Transition {
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

fn on_editing_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    let field = state.form.field_mut(state.field_focus);
    match field {
        FieldState::Text(text) => {
            // Esc commits and exits — there is no separate "cancel" for
            // textareas; if the user wants to discard a change they can
            // re-enter and overwrite. Enter commits a single-line field;
            // in multiline it inserts a newline as expected.
            let multiline = text.multiline;
            let commit = key.code == KeyCode::Esc || (!multiline && key.code == KeyCode::Enter);
            if commit {
                text.commit();
                state.input_mode = InputMode::Normal;
            } else {
                text.input(key);
            }
            Transition::Dirty
        }
        FieldState::Picker(picker) => {
            match key.code {
                KeyCode::Esc => {
                    picker.cancel();
                    state.input_mode = InputMode::Normal;
                }
                KeyCode::Enter => {
                    picker.commit();
                    state.input_mode = InputMode::Normal;
                }
                KeyCode::Char(' ') if picker.multi_select => picker.toggle_highlighted(),
                KeyCode::Up => picker.move_highlight(-1),
                KeyCode::Down => picker.move_highlight(1),
                KeyCode::Char('k') if key.modifiers.is_empty() => picker.move_highlight(-1),
                KeyCode::Char('j') if key.modifiers.is_empty() => picker.move_highlight(1),
                KeyCode::Char(_) | KeyCode::Backspace => picker.input_filter(key),
                _ => {}
            }
            Transition::Dirty
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GeneratedDraft, PromptBuild, PromptManifest};
    use crate::screens::generate::PrForm;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
}
