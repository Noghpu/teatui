use serde_json::Value;

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
