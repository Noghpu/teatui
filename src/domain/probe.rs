use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::config::LlmApi;
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

/// Health probe for a single named backend. The result carries the
/// backend `name` so the app can route it to the right row when probing
/// several backends at once (the backend switcher fires one per backend).
pub struct BackendHealthProbe {
    pub name: String,
    pub base_url: String,
    pub api: LlmApi,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

/// An `LlmHealth` tagged with the backend it describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendHealth {
    pub name: String,
    pub health: LlmHealth,
}

impl Job for BackendHealthProbe {
    fn name(&self) -> &'static str {
        "probe.llm.health"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let health = check_llm_health(
            &self.base_url,
            self.api,
            self.api_key.as_deref(),
            self.timeout,
        );
        JobOutcome::Done(Box::new(BackendHealth {
            name: self.name,
            health,
        }))
    }
}

/// Hit the backend's model-listing endpoint and classify the outcome as
/// reachable (with the model list) or unreachable (with the transport /
/// parse error). The endpoint and response shape depend on the protocol:
/// Ollama serves `/api/tags`; OpenAI-compatible servers serve `/v1/models`.
fn check_llm_health(
    base_url: &str,
    api: LlmApi,
    api_key: Option<&str>,
    timeout: Duration,
) -> LlmHealth {
    let base = base_url.trim_end_matches('/');
    let url = match api {
        LlmApi::Ollama => format!("{base}/api/tags"),
        LlmApi::Openai => format!("{base}/v1/models"),
    };
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut request = agent.get(&url);
    if let Some(key) = api_key {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    match request.call() {
        Ok(mut resp) => {
            let models = match api {
                LlmApi::Ollama => resp
                    .body_mut()
                    .read_json::<TagsResponse>()
                    .map(|t| t.models.into_iter().map(|m| m.name).collect()),
                LlmApi::Openai => resp
                    .body_mut()
                    .read_json::<ModelsResponse>()
                    .map(|m| m.data.into_iter().map(|x| x.id).collect()),
            };
            match models {
                Ok(models) => LlmHealth::Available { models },
                Err(e) => LlmHealth::Unreachable {
                    message: format!("invalid response: {e}"),
                },
            }
        }
        Err(e) => LlmHealth::Unreachable {
            message: e.to_string(),
        },
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

/// OpenAI `/v1/models` response: `{ "data": [ { "id": "..." }, ... ] }`.
#[derive(Debug, serde::Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelItem>,
}

#[derive(Debug, serde::Deserialize)]
struct ModelItem {
    id: String,
}

// =========================== Revsets ========================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevsetSummary {
    pub label: String,
    pub change_id: String,
    pub commit_id: String,
    pub bookmarks: Vec<String>,
    pub description: String,
    pub description_body: String,
    pub author: String,
    pub stats: String,
    pub commit_count: usize,
    pub commit_ids: Vec<String>,
    pub change_ids: Vec<String>,
    pub recent_log: Vec<String>,
    pub warnings: Vec<String>,
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
            revset: "trunk()..@".into(),
        }
    }
}

impl Job for RevsetProbe {
    fn name(&self) -> &'static str {
        "probe.revsets"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        // Fast list-only probe — no diff computation. Stats are
        // populated incrementally by `RevsetStatsProbe`, which fires
        // after this one completes (see `App::absorb_payload`). Splits
        // the first paint of the Changes pane (~0.6s) from the heavier
        // stats fill-in (~1.4s, deferred).
        //
        // Each record starts with `\x1E`, then `commit|change|bookmarks|
        // description` (description body lines joined by `\x1F`), then
        // `\x1D\n` to mark end-of-record. The parser tolerates a missing
        // diff-stat block — we always read records that way.
        const TEMPLATE: &str = r#""\x1E" ++ commit_id.short() ++ "|" ++ change_id.short() ++ "|" ++ bookmarks.map(|b| b.name()).join(",") ++ "|" ++ description.lines().join("\x1F") ++ "\x1D\n""#;
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
                let summaries = parse_revset_log_with_stats(&stdout);
                Revsets::Loaded(summaries)
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

/// Deferred diff-stat fetcher. Issues a single `jj log --stat` for the
/// same revset, parses out the summary line for each change, and returns
/// the result as `RevsetStats` keyed by `change_id`. Launched after
/// `RevsetProbe` completes so the list paints first.
pub struct RevsetStatsProbe {
    pub jj_binary: String,
    pub revset: String,
}

/// Per-change-id diff-stat summary lines, merged into existing
/// `RevsetSummary` entries by `App::absorb_payload`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RevsetStats(pub Vec<(String, String)>);

impl Job for RevsetStatsProbe {
    fn name(&self) -> &'static str {
        "probe.revset_stats"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        // Same template, with `--stat` so jj appends the diff block
        // after each record. The summary line is what we keep.
        const TEMPLATE: &str = r#""\x1E" ++ commit_id.short() ++ "|" ++ change_id.short() ++ "|" ++ bookmarks.map(|b| b.name()).join(",") ++ "|" ++ description.lines().join("\x1F") ++ "\x1D\n""#;
        let result = match Command::new(&self.jj_binary)
            .args([
                "--no-pager",
                "log",
                "-r",
                &self.revset,
                "--no-graph",
                "--stat",
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
                let pairs = parse_revset_log_with_stats(&stdout)
                    .into_iter()
                    .map(|s| (s.change_id, s.stats))
                    .collect();
                RevsetStats(pairs)
            }
            // Failures fall through silently — first paint already
            // succeeded; missing stats just leave rows without the
            // compact "4f +188 -34" metadata line.
            _ => RevsetStats::default(),
        };
        JobOutcome::Done(Box::new(result))
    }
}

#[derive(Debug)]
struct ParsedRevsetEntry {
    commit_id: String,
    change_id: String,
    bookmarks: Vec<String>,
    description: String,
    description_body: String,
}

fn parse_revset_log_entry(line: &str) -> Option<ParsedRevsetEntry> {
    let mut fields = line.splitn(4, '|');
    let commit_id = fields.next()?.to_string();
    let change_id = fields.next()?.to_string();
    let bookmarks_raw = fields.next().unwrap_or("");
    let description_raw = fields.next().unwrap_or("");

    let bookmarks = if bookmarks_raw.is_empty() {
        Vec::new()
    } else {
        bookmarks_raw.split(',').map(str::to_string).collect()
    };
    let mut description_lines = description_raw.split('\x1F');
    let description = description_lines.next().unwrap_or("").to_string();
    let description_body = description_lines.collect::<Vec<_>>().join("\n");

    Some(ParsedRevsetEntry {
        commit_id,
        change_id,
        bookmarks,
        description,
        description_body,
    })
}

/// Parse the combined `jj log --stat` output produced by the template
/// in `RevsetProbe`. Each record starts with `\x1E`, holds metadata up
/// to `\x1D`, then jj's `--stat` block (file lines + summary) until the
/// next record. We discard the per-file lines and keep the summary.
fn parse_revset_log_with_stats(stdout: &str) -> Vec<RevsetSummary> {
    let mut out = Vec::new();
    for record in stdout.split('\x1E').skip(1) {
        let Some((meta_part, stat_part)) = record.split_once('\x1D') else {
            continue;
        };
        let Some(entry) = parse_revset_log_entry(meta_part.trim_end_matches('\n')) else {
            continue;
        };
        let stats = extract_diff_stat_summary(stat_part);
        out.push(summary_from_entry(entry, stats));
    }
    out
}

/// Pull the "N files changed, X insertions(+), Y deletions(-)" line
/// from a jj `--stat` block.
fn extract_diff_stat_summary(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && l.contains("file") && l.contains("changed"))
        .unwrap_or("")
        .to_string()
}

fn summary_from_entry(entry: ParsedRevsetEntry, stats: String) -> RevsetSummary {
    let label = format!("trunk()..{}", entry.change_id);
    let recent_log = vec![format!(
        "{} {}",
        entry.commit_id,
        non_empty_or(&entry.description, "(no description set)")
    )];
    RevsetSummary {
        label,
        change_id: entry.change_id.clone(),
        commit_id: entry.commit_id.clone(),
        bookmarks: entry.bookmarks,
        description: entry.description,
        description_body: entry.description_body,
        author: String::new(),
        stats,
        commit_count: 1,
        commit_ids: vec![entry.commit_id],
        change_ids: vec![entry.change_id],
        recent_log,
        warnings: Vec::new(),
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
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
        let raw = "\x1Edeadbeef|abcd1234|feature/foo,bar|First line of desc\x1FBody line\x1D\nsrc/foo.rs | 10 ++++++\n1 file changed, 8 insertions(+), 2 deletions(-)\n\x1Eef567890|cafebabe||Another change\x1D\n0 files changed, 0 insertions(+), 0 deletions(-)\n";
        let revsets = parse_revset_log_with_stats(raw);
        assert_eq!(revsets.len(), 2);
        assert_eq!(revsets[0].change_id, "abcd1234");
        assert_eq!(revsets[0].commit_id, "deadbeef");
        assert_eq!(
            revsets[0].bookmarks,
            vec!["feature/foo".to_string(), "bar".to_string()]
        );
        assert_eq!(revsets[0].description, "First line of desc");
        assert_eq!(revsets[0].description_body, "Body line");
        assert_eq!(revsets[0].label, "trunk()..abcd1234");
        assert_eq!(revsets[0].commit_ids, vec!["deadbeef".to_string()]);
        assert_eq!(revsets[0].change_ids, vec!["abcd1234".to_string()]);
        assert_eq!(
            revsets[0].stats,
            "1 file changed, 8 insertions(+), 2 deletions(-)"
        );
        assert!(revsets[1].bookmarks.is_empty());
        assert_eq!(
            revsets[1].stats,
            "0 files changed, 0 insertions(+), 0 deletions(-)"
        );
        assert_eq!(revsets[1].author, "");
    }

    #[test]
    fn parses_revsets_handles_empty_input() {
        let revsets = parse_revset_log_with_stats("");
        assert!(revsets.is_empty());
    }

    #[test]
    fn parses_revsets_handles_entry_without_stats() {
        // Defensive: if jj ever drops the summary line, the entry still
        // parses with an empty `stats` rather than being dropped.
        let raw = "\x1Eabc123|change1||desc\x1D\n";
        let revsets = parse_revset_log_with_stats(raw);
        assert_eq!(revsets.len(), 1);
        assert_eq!(revsets[0].stats, "");
    }
}
