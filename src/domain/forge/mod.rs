pub mod gitea;
pub mod github;

use super::process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    Gitea,
    Github,
}

impl ForgeKind {
    pub fn label(self) -> &'static str {
        match self {
            ForgeKind::Gitea => "tea",
            ForgeKind::Github => "gh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeCli {
    kind: ForgeKind,
    binary: String,
    host: Option<String>,
}

impl ForgeCli {
    pub fn new(kind: ForgeKind, binary: String, host: Option<String>) -> Self {
        Self { kind, binary, host }
    }

    pub fn kind(&self) -> ForgeKind {
        self.kind
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }

    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    pub fn auth_status(&self) -> ForgeAuthStatus {
        match self.kind {
            ForgeKind::Gitea => gitea::Driver::auth_status(self),
            ForgeKind::Github => github::Driver::auth_status(self),
        }
    }

    pub fn repo_options(&self, owner: &str, repo: &str) -> RepoOptions {
        match self.kind {
            ForgeKind::Gitea => gitea::Driver::repo_options(self, owner, repo),
            ForgeKind::Github => github::Driver::repo_options(self, owner, repo),
        }
    }

    pub fn existing_prs(&self, owner: Option<&str>, repo: Option<&str>) -> StackExistingPrs {
        match self.kind {
            ForgeKind::Gitea => gitea::Driver::existing_prs(self, owner, repo),
            ForgeKind::Github => github::Driver::existing_prs(self, owner, repo),
        }
    }

    pub fn create_args(&self, input: &CreatePrInput<'_>) -> Vec<String> {
        match self.kind {
            ForgeKind::Gitea => gitea::Driver::create_args(input),
            ForgeKind::Github => github::Driver::create_args(input),
        }
    }

    pub fn create_pr(&self, input: &CreatePrInput<'_>) -> Result<String, String> {
        process::capture(self.binary(), &self.create_args(input))
    }
}

pub(crate) trait ForgeDriver {
    fn auth_status(cli: &ForgeCli) -> ForgeAuthStatus;
    fn repo_options(cli: &ForgeCli, owner: &str, repo: &str) -> RepoOptions;
    fn existing_prs(cli: &ForgeCli, owner: Option<&str>, repo: Option<&str>) -> StackExistingPrs;
    fn create_args(input: &CreatePrInput<'_>) -> Vec<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeAuthStatus {
    Configured { logins: Vec<String> },
    None,
    Errored { message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoOptions {
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestones: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackExistingPr {
    pub head_branch: String,
    pub state: String,
    pub url: Option<String>,
}

pub type StackExistingPrs = Vec<StackExistingPr>;

pub struct CreatePrInput<'a> {
    pub base: &'a str,
    pub head: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub labels: &'a [String],
    pub assignees: &'a [String],
    pub milestone: &'a str,
}

pub(crate) fn parse_names(stdout: &str) -> Vec<String> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    collect_name_items(&root)
        .into_iter()
        .filter_map(|item| item.name.or(item.login).or(item.title))
        .collect()
}

fn collect_name_items(root: &serde_json::Value) -> Vec<NameItem> {
    match root {
        serde_json::Value::Array(items) => items
            .iter()
            .flat_map(|item| match item {
                serde_json::Value::Array(_) => collect_name_items(item),
                _ => serde_json::from_value::<NameItem>(item.clone())
                    .map(|item| vec![item])
                    .unwrap_or_default(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct NameItem {
    name: Option<String>,
    login: Option<String>,
    title: Option<String>,
}

pub(crate) fn parse_existing_prs(stdout: &str) -> StackExistingPrs {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    parse_existing_prs_value(&root)
}

fn parse_existing_prs_value(root: &serde_json::Value) -> StackExistingPrs {
    let Some(items) = root.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .flat_map(|item| {
            item.as_array()
                .map(|_| parse_existing_prs_value(item))
                .unwrap_or_else(|| parse_existing_pr_item(item).into_iter().collect())
        })
        .collect()
}

fn parse_existing_pr_item(item: &serde_json::Value) -> Option<StackExistingPr> {
    let obj = item.as_object()?;
    let head_branch = field_string(
        obj,
        &[
            "head_branch",
            "headBranch",
            "head_ref",
            "headRefName",
            "source_branch",
            "sourceBranch",
            "branch",
            "head",
        ],
    )?;
    let state = field_string(obj, &["state", "status"]).unwrap_or_default();
    let url = field_string(obj, &["url", "html_url", "href", "web_url"]);
    Some(StackExistingPr {
        head_branch,
        state,
        url,
    })
}

fn field_string(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = obj.get(*key)
            && let Some(text) = json_string(value)
        {
            return Some(text);
        }
    }
    None
}

fn json_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let text = s.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Array(items) => items.iter().find_map(json_string),
        serde_json::Value::Object(map) => {
            for key in [
                "ref",
                "name",
                "label",
                "title",
                "value",
                "url",
                "text",
                "state",
                "head_branch",
            ] {
                if let Some(value) = map.get(key)
                    && let Some(text) = json_string(value)
                {
                    return Some(text);
                }
            }
            map.values().find_map(json_string)
        }
        serde_json::Value::Null => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_existing_prs_tolerates_string_and_object_fields() {
        let raw = r#"
        [
          {
            "head_branch": "pr/feat/add-foo",
            "state": "open",
            "url": "https://example.com/pulls/1",
            "ignored": {"nested": true}
          },
          {
            "head_branch": {"name": "pr/fix/rework"},
            "state": {"title": "merged"},
            "url": {"html_url": "https://example.com/pulls/2"}
          },
          {
            "head": {"ref": "pr/chore/nested-head", "label": "owner:pr/chore/nested-head"},
            "status": "closed",
            "html_url": "https://example.com/pulls/3",
            "title": "Do not parse title as a branch"
          },
          {
            "state": "open",
            "title": "Do not parse title-only entries"
          }
        ]
        "#;
        let prs = parse_existing_prs(raw);
        assert_eq!(prs.len(), 3);
        assert_eq!(prs[0].head_branch, "pr/feat/add-foo");
        assert_eq!(prs[0].state, "open");
        assert_eq!(prs[0].url.as_deref(), Some("https://example.com/pulls/1"));
        assert_eq!(prs[1].head_branch, "pr/fix/rework");
        assert_eq!(prs[1].state, "merged");
        assert_eq!(prs[1].url.as_deref(), Some("https://example.com/pulls/2"));
        assert_eq!(prs[2].head_branch, "pr/chore/nested-head");
        assert_eq!(prs[2].state, "closed");
        assert_eq!(prs[2].url.as_deref(), Some("https://example.com/pulls/3"));
    }

    #[test]
    fn parses_existing_prs_flattens_paginated_gh_api_arrays() {
        let raw = r#"
        [
          [
            {"head": {"ref": "pr/one"}, "state": "open", "html_url": "https://github.com/o/r/pull/1"}
          ],
          [
            {"head": {"ref": "pr/two"}, "state": "closed", "html_url": "https://github.com/o/r/pull/2"}
          ]
        ]
        "#;
        let prs = parse_existing_prs(raw);
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].head_branch, "pr/one");
        assert_eq!(prs[1].head_branch, "pr/two");
    }

    #[test]
    fn parses_existing_prs_handles_malformed_json() {
        let prs = parse_existing_prs("{ definitely not json");
        assert!(prs.is_empty());
    }

    #[test]
    fn parses_names_flattens_paginated_arrays() {
        let raw = r#"
        [
          [{"name": "bug"}, {"login": "alice"}],
          [{"title": "v1"}]
        ]
        "#;
        assert_eq!(parse_names(raw), vec!["bug", "alice", "v1"]);
    }
}
