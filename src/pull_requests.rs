use serde_json::Value;

use crate::generate::{ScrollState, TextFieldState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrCommentPhase {
    #[default]
    Idle,
    Editing,
    Submitting,
    Failed,
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
    pub preview_scroll: ScrollState,
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
            preview_scroll: ScrollState::default(),
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
pub struct PullRequestSummary {
    pub index: u64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub url: String,
    pub head: String,
    pub base: String,
    pub body: String,
    pub updated: String,
    pub labels: Vec<String>,
}

pub fn parse_pull_requests_json(json: &str) -> Vec<PullRequestSummary> {
    let value = match serde_json::from_str::<Value>(json) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let Value::Array(items) = value else {
        return Vec::new();
    };

    items
        .into_iter()
        .filter_map(parse_pull_request_value)
        .collect()
}

pub fn parse_pull_request_json(json: &str) -> Option<PullRequestSummary> {
    let value = serde_json::from_str::<Value>(json).ok()?;
    parse_pull_request_value(value)
}

fn parse_pull_request_value(value: Value) -> Option<PullRequestSummary> {
    let Value::Object(map) = value else {
        return None;
    };
    let index = parse_u64(map.get("index"))?;

    Some(PullRequestSummary {
        index,
        title: parse_string_or_default(map.get("title")),
        state: parse_string_or_default(map.get("state")),
        author: parse_author(map.get("author")).unwrap_or_default(),
        url: parse_string_or_default(map.get("url")),
        head: parse_string_or_default(map.get("head")),
        base: parse_string_or_default(map.get("base")),
        body: parse_string_or_default(map.get("body")),
        updated: parse_string_or_default(map.get("updated")),
        labels: parse_labels(map.get("labels")),
    })
}

fn parse_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn parse_string(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::String(text) => text.trim(),
        _ => return None,
    };

    (!text.is_empty()).then(|| text.to_string())
}

fn parse_string_or_default(value: Option<&Value>) -> String {
    parse_string(value).unwrap_or_default()
}

fn parse_author(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Object(map) => first_non_empty_string(&[
            map.get("login"),
            map.get("name"),
            map.get("full_name"),
            map.get("display_name"),
            map.get("username"),
        ]),
        _ => None,
    }
}

fn parse_labels(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };

    items.iter().filter_map(parse_label_value).collect()
}

fn parse_label_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Object(map) => first_non_empty_string(&[map.get("name"), map.get("title")]),
        _ => None,
    }
}

fn first_non_empty_string(values: &[Option<&Value>]) -> Option<String> {
    for value in values {
        if let Some(text) = parse_string(*value) {
            return Some(text);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

    #[test]
    fn parses_pull_requests_json_with_mixed_value_shapes() {
        let json = r##"[
          {
            "index": 42,
            "title": "Add feature",
            "state": "open",
            "author": "alice",
            "url": "https://example.com/pr/42",
            "head": "feature/add-feature",
            "base": "main",
            "body": "Body text",
            "updated": "2026-05-29T10:00:00Z",
            "labels": ["bug", {"name": "docs"}],
            "extra_field": "ignored"
          },
          {
            "index": "7",
            "title": "Fix issue",
            "state": "draft",
            "author": {"login": "bob", "display_name": "Bob Smith"},
            "url": "https://example.com/pr/7",
            "head": "fix/issue",
            "base": "main",
            "body": null,
            "updated": null,
            "labels": [{"title": "needs-review"}, {"name": "ui"}]
          }
        ]"##;

        let prs = parse_pull_requests_json(json);
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].index, 42);
        assert_eq!(prs[0].author, "alice");
        assert_eq!(prs[0].labels, vec!["bug", "docs"]);
        assert_eq!(prs[1].index, 7);
        assert_eq!(prs[1].author, "bob");
        assert_eq!(prs[1].labels, vec!["needs-review", "ui"]);
        assert_eq!(prs[1].body, "");
        assert_eq!(prs[1].updated, "");
    }

    #[test]
    fn parses_single_pull_request_json() {
        let json = r##"{
          "index": "11",
          "title": "Improve docs",
          "state": "merged",
          "author": {"name": "Carol"},
          "url": "https://example.com/pr/11",
          "head": "docs/improve",
          "base": "main",
          "body": "Detailed body",
          "updated": "2026-05-28T09:30:00Z",
          "labels": [{"name": "docs"}]
        }"##;

        let pr = parse_pull_request_json(json).expect("pr");
        assert_eq!(pr.index, 11);
        assert_eq!(pr.author, "Carol");
        assert_eq!(pr.body, "Detailed body");
        assert_eq!(pr.labels, vec!["docs"]);
    }

    #[test]
    fn returns_none_for_invalid_or_incomplete_json() {
        assert!(parse_pull_request_json("not json").is_none());
        assert!(parse_pull_request_json(r#"{"title": "missing index"}"#).is_none());
        assert!(parse_pull_requests_json("not json").is_empty());
        assert!(parse_pull_requests_json(r#"{"index": 1}"#).is_empty());
    }

    #[test]
    fn tolerates_missing_string_fields_in_list_entries() {
        let json = r##"[
          {
            "index": 1,
            "title": null,
            "state": null,
            "author": null
          }
        ]"##;

        let prs = parse_pull_requests_json(json);
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].title, "");
        assert_eq!(prs[0].state, "");
        assert_eq!(prs[0].author, "");
        assert_eq!(prs[0].url, "");
        assert_eq!(prs[0].head, "");
        assert_eq!(prs[0].base, "");
        assert_eq!(prs[0].body, "");
        assert_eq!(prs[0].updated, "");
        assert!(prs[0].labels.is_empty());
    }
}
