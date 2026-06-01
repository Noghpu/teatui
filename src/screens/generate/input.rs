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
        (KeyCode::Esc, _) => Transition::Navigate(NewScreen::Landing),
        (KeyCode::Tab, _) => {
            state.pane = state.pane.next();
            Transition::Dirty
        }
        (KeyCode::BackTab, _) => {
            state.pane = state.pane.prev();
            Transition::Dirty
        }
        (KeyCode::Char('g'), false) if !state.is_in_progress() => {
            if state.form.validate() {
                Transition::Generate
            } else {
                Transition::Dirty
            }
        }
        (KeyCode::Char('x'), false) if matches!(state.phase, GeneratePhase::DraftReady { .. }) => {
            if state.form.validate() {
                Transition::Execute
            } else {
                Transition::Dirty
            }
        }
        (KeyCode::Char('c'), false) if state.done_url().is_some() => Transition::CopyUrl,
        (KeyCode::Char('o'), false) if state.done_url().is_some() => Transition::OpenUrl,
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
        _ => Transition::None,
    }
}

fn on_editing_key(state: &mut GenerateState, key: KeyEvent) -> Transition {
    let field = state.form.field_mut(state.field_focus);
    match field {
        FieldState::Text(text) => {
            let multiline = text.multiline;
            let commit = (!multiline && key.code == KeyCode::Enter)
                || (multiline
                    && ((key.code == KeyCode::Char('s')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                        || (key.code == KeyCode::Enter
                            && key.modifiers.contains(KeyModifiers::ALT))));
            if key.code == KeyCode::Esc {
                text.cancel();
                state.input_mode = InputMode::Normal;
            } else if commit {
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
