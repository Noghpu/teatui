use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::runtime::{Job, JobOutcome};

// ============================== Version =====================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionKind {
    Jj,
    Git,
    Tea,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Available { version: String },
    Missing,
    Errored { message: String },
}

#[derive(Debug, Clone)]
pub struct VersionResult {
    pub kind: VersionKind,
    pub status: ToolStatus,
}

pub struct VersionProbe {
    pub kind: VersionKind,
    pub binary: String,
}

impl Job for VersionProbe {
    fn name(&self) -> &'static str {
        match self.kind {
            VersionKind::Jj => "probe.jj.version",
            VersionKind::Git => "probe.git.version",
            VersionKind::Tea => "probe.tea.version",
        }
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let status = run_version_check(&self.binary);
        JobOutcome::Done(Box::new(VersionResult {
            kind: self.kind,
            status,
        }))
    }
}

fn run_version_check(binary: &str) -> ToolStatus {
    match Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            ToolStatus::Available { version }
        }
        Ok(out) => ToolStatus::Errored {
            message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ToolStatus::Missing,
        Err(e) => ToolStatus::Errored {
            message: e.to_string(),
        },
    }
}

// ============================ Workspace =====================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceInfo {
    Inside {
        root: PathBuf,
        remote: Option<RemoteInfo>,
    },
    Outside,
    Errored {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInfo {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

pub struct WorkspaceProbe {
    pub jj_binary: String,
}

impl Job for WorkspaceProbe {
    fn name(&self) -> &'static str {
        "probe.workspace"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = match Command::new(&self.jj_binary)
            .args(["workspace", "root"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(out) if out.status.success() => {
                let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let remote = origin_remote_url(&self.jj_binary)
                    .as_deref()
                    .and_then(parse_remote_info);
                WorkspaceInfo::Inside {
                    root: PathBuf::from(root),
                    remote,
                }
            }
            Ok(_) => WorkspaceInfo::Outside,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => WorkspaceInfo::Errored {
                message: format!("{} not found", self.jj_binary),
            },
            Err(e) => WorkspaceInfo::Errored {
                message: e.to_string(),
            },
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn origin_remote_url(jj_binary: &str) -> Option<String> {
    let out = Command::new(jj_binary)
        .args(["git", "remote", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let name = parts.next()?;
        let url = parts.next()?;
        (name == "origin").then(|| url.to_string())
    })
}

fn parse_remote_info(url: &str) -> Option<RemoteInfo> {
    let normalized = url
        .trim()
        .trim_end_matches(".git")
        .replace(':', "/")
        .replace("git@", "");
    let without_scheme = normalized
        .strip_prefix("https://")
        .or_else(|| normalized.strip_prefix("http://"))
        .or(Some(normalized.as_str()))?;
    let parts: Vec<&str> = without_scheme
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    let host = parts.first()?.to_string();
    let owner = parts.get(parts.len().checked_sub(2)?)?.to_string();
    let repo = parts.last()?.to_string();
    Some(RemoteInfo { host, owner, repo })
}

// =========================== Tea auth =======================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeaAuthStatus {
    Configured { logins: Vec<String> },
    None,
    Errored { message: String },
}

pub struct TeaAuthProbe {
    pub tea_binary: String,
}

impl Job for TeaAuthProbe {
    fn name(&self) -> &'static str {
        "probe.tea.auth"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = match Command::new(&self.tea_binary)
            .args(["login", "list"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let logins = parse_tea_logins(&stdout);
                if logins.is_empty() {
                    TeaAuthStatus::None
                } else {
                    TeaAuthStatus::Configured { logins }
                }
            }
            Ok(out) => TeaAuthStatus::Errored {
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => TeaAuthStatus::Errored {
                message: format!("{} not found", self.tea_binary),
            },
            Err(e) => TeaAuthStatus::Errored {
                message: e.to_string(),
            },
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn parse_tea_logins(stdout: &str) -> Vec<String> {
    // Output format (whitespace-aligned table):
    //   Name      URL                                Default
    //   gitea     https://gitea.example.com          *
    // Skip the header row, take the first column.
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let _ = lines.next();
    lines
        .map(|line| line.split_whitespace().next().unwrap_or("").to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

// =========================== LLM health =====================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmHealth {
    Available { models: Vec<String> },
    Unreachable { message: String },
}

pub struct LlmHealthProbe {
    pub base_url: String,
    pub timeout: Duration,
}

impl Job for LlmHealthProbe {
    fn name(&self) -> &'static str {
        "probe.llm.health"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let result = match agent.get(&url).call() {
            Ok(mut resp) => match resp.body_mut().read_json::<TagsResponse>() {
                Ok(tags) => LlmHealth::Available {
                    models: tags.models.into_iter().map(|m| m.name).collect(),
                },
                Err(e) => LlmHealth::Unreachable {
                    message: format!("invalid response: {e}"),
                },
            },
            Err(e) => LlmHealth::Unreachable {
                message: e.to_string(),
            },
        };
        JobOutcome::Done(Box::new(result))
    }
}

#[derive(Debug, serde::Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Debug, serde::Deserialize)]
struct TagModel {
    name: String,
}

// =========================== Revsets ========================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevsetSummary {
    pub change_id: String,
    pub commit_id: String,
    pub bookmarks: Vec<String>,
    pub description: String,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revsets {
    Loaded(Vec<RevsetSummary>),
    Errored { message: String },
}

pub struct RevsetProbe {
    pub jj_binary: String,
    pub revset: String,
}

impl Default for RevsetProbe {
    fn default() -> Self {
        Self {
            jj_binary: "jj".into(),
            revset: "mutable()".into(),
        }
    }
}

impl Job for RevsetProbe {
    fn name(&self) -> &'static str {
        "probe.revsets"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        const TEMPLATE: &str = r#"change_id.short() ++ "\t" ++ commit_id.short() ++ "\t" ++ bookmarks.map(|b| b.name()).join(",") ++ "\t" ++ description.first_line() ++ "\t" ++ author.name() ++ "\n""#;
        let result = match Command::new(&self.jj_binary)
            .args([
                "--no-pager",
                "log",
                "-r",
                &self.revset,
                "--no-graph",
                "-T",
                TEMPLATE,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                Revsets::Loaded(parse_revsets(&stdout))
            }
            Ok(out) => Revsets::Errored {
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Revsets::Errored {
                message: format!("{} not found", self.jj_binary),
            },
            Err(e) => Revsets::Errored {
                message: e.to_string(),
            },
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn parse_revsets(stdout: &str) -> Vec<RevsetSummary> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\t');
            let change_id = fields.next()?.to_string();
            let commit_id = fields.next()?.to_string();
            let bookmarks_raw = fields.next()?;
            let description = fields.next()?.to_string();
            let author = fields.next().unwrap_or("").to_string();
            let bookmarks = if bookmarks_raw.is_empty() {
                Vec::new()
            } else {
                bookmarks_raw.split(',').map(|s| s.to_string()).collect()
            };
            Some(RevsetSummary {
                change_id,
                commit_id,
                bookmarks,
                description,
                author,
            })
        })
        .collect()
}

// ======================== Base bookmarks ===================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseBookmark {
    pub name: String,
    pub remote: Option<String>,
    pub is_remote: bool,
}

pub type BaseBookmarks = Vec<BaseBookmark>;

pub struct BaseBookmarksProbe {
    pub jj_binary: String,
}

impl Job for BaseBookmarksProbe {
    fn name(&self) -> &'static str {
        "probe.base_bookmarks"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        const TEMPLATE: &str = r#"name() ++ "\t" ++ remote().name() ++ "\n""#;
        let bookmarks = match Command::new(&self.jj_binary)
            .args([
                "--no-pager",
                "bookmark",
                "list",
                "--all-remotes",
                "-T",
                TEMPLATE,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                parse_base_bookmarks(&stdout)
            }
            _ => Vec::new(),
        };
        JobOutcome::Done(Box::new(bookmarks))
    }
}

fn parse_base_bookmarks(stdout: &str) -> BaseBookmarks {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let name = parts.next()?.trim().trim_end_matches('@').to_string();
            let remote = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            Some(BaseBookmark {
                is_remote: remote.is_some() || name.contains('@'),
                name,
                remote,
            })
        })
        .collect()
}

// ======================== Repo options =====================================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoOptions {
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestones: Vec<String>,
}

pub struct RepoOptionsProbe {
    pub tea_binary: String,
    pub owner: String,
    pub repo: String,
}

impl Job for RepoOptionsProbe {
    fn name(&self) -> &'static str {
        "probe.repo_options"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        let labels = tea_names(
            &self.tea_binary,
            &format!("repos/{}/{}/labels", self.owner, self.repo),
        );
        let assignees = tea_names(
            &self.tea_binary,
            &format!("repos/{}/{}/collaborators", self.owner, self.repo),
        );
        let milestones = tea_names(
            &self.tea_binary,
            &format!("repos/{}/{}/milestones", self.owner, self.repo),
        );
        JobOutcome::Done(Box::new(RepoOptions {
            labels,
            assignees,
            milestones,
        }))
    }
}

fn tea_names(binary: &str, path: &str) -> Vec<String> {
    let Ok(out) = Command::new(binary)
        .args(["api", path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    serde_json::from_slice::<Vec<NameItem>>(&out.stdout)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.name.or(item.login).or(item.title))
        .collect()
}

#[derive(Debug, serde::Deserialize)]
struct NameItem {
    name: Option<String>,
    login: Option<String>,
    title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tea_login_list_with_one_login() {
        let raw = "Name      URL                                Default\ngitea     https://gitea.example.com         *\n";
        let logins = parse_tea_logins(raw);
        assert_eq!(logins, vec!["gitea".to_string()]);
    }

    #[test]
    fn parses_tea_login_list_with_no_logins() {
        let raw = "Name      URL                                Default\n";
        let logins = parse_tea_logins(raw);
        assert!(logins.is_empty());
    }

    #[test]
    fn parses_tea_login_list_with_multiple() {
        let raw = "Name    URL                          Default\ngitea   https://gitea.example.com    *\nother   https://other.example.com\n";
        let logins = parse_tea_logins(raw);
        assert_eq!(logins, vec!["gitea".to_string(), "other".to_string()]);
    }

    #[test]
    fn parses_revsets_with_bookmarks_and_empty_bookmarks() {
        let raw = "abcd1234\tdeadbeef\tfeature/foo,bar\tFirst line of desc\tAlice\nef567890\tcafebabe\t\tAnother change\tBob\n";
        let revsets = parse_revsets(raw);
        assert_eq!(revsets.len(), 2);
        assert_eq!(revsets[0].change_id, "abcd1234");
        assert_eq!(
            revsets[0].bookmarks,
            vec!["feature/foo".to_string(), "bar".to_string()]
        );
        assert_eq!(revsets[0].description, "First line of desc");
        assert_eq!(revsets[0].author, "Alice");
        assert!(revsets[1].bookmarks.is_empty());
        assert_eq!(revsets[1].author, "Bob");
    }

    #[test]
    fn parses_revsets_ignores_blank_lines() {
        let raw = "abcd\tef01\t\tdesc\tA\n\n\n";
        let revsets = parse_revsets(raw);
        assert_eq!(revsets.len(), 1);
    }
}
