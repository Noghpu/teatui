use std::path::{Path, PathBuf};
use std::time::Duration;

use super::forge::{ForgeCli, StackExistingPrs};
use super::process;
use crate::config::LlmApi;
use crate::runtime::{Job, JobOutcome};

// ============================== Version =====================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionKind {
    Jj,
    Git,
    Forge,
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
            VersionKind::Forge => "probe.forge.version",
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
    match process::output(binary, &["--version"]) {
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
        /// Git remote name used for push (e.g. "origin", "gitea"). Read from
        /// .git/config directly so it's available even when git safe.directory
        /// blocks process-based git access.
        remote_name: Option<String>,
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
        let result = match process::jj_output(&self.jj_binary, &["workspace", "root"]) {
            Ok(out) if out.status.success() => {
                let root = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
                let (remote, remote_name) =
                    match remote_url_from_colocated_git_config(&root) {
                        Some((name, url)) => (parse_remote_info(&url), Some(name)),
                        None => (None, None),
                    };
                WorkspaceInfo::Inside { root, remote, remote_name }
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

fn remote_url_from_colocated_git_config(root: &Path) -> Option<(String, String)> {
    let target_raw = std::fs::read_to_string(root.join(".jj/repo/store/git_target")).ok()?;
    let git_target = root.join(".jj/repo/store").join(target_raw.trim());
    let config = std::fs::read_to_string(git_target.join("config")).ok()?;
    remote_url_from_git_config(&config)
}

/// Returns `(remote_name, url)` for the best remote found in a raw git config.
/// Prefers "origin"; falls back to the first remote with a parseable URL.
fn remote_url_from_git_config(config: &str) -> Option<(String, String)> {
    let mut current_remote = None::<String>;
    let mut fallback = None;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_remote = parse_git_remote_section(trimmed);
            continue;
        }
        let Some(remote) = current_remote.as_deref() else {
            continue;
        };
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "url" {
            continue;
        }
        let url = value.trim().to_string();
        if fallback.is_none() && parse_remote_info(&url).is_some() {
            fallback = Some((remote.to_string(), url.clone()));
        }
        if remote == "origin" {
            return Some(("origin".to_string(), url));
        }
    }
    fallback
}

fn parse_git_remote_section(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    let name = inner.strip_prefix("remote ")?.trim();
    Some(name.trim_matches('"').to_string())
}

fn parse_remote_info(url: &str) -> Option<RemoteInfo> {
    let trimmed = url.trim().trim_end_matches(".git");
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("ssh://git@"))
        .or_else(|| trimmed.strip_prefix("ssh://"));
    let pathish = match without_scheme {
        Some(rest) => rest.to_string(),
        None => trimmed
            .strip_prefix("git@")
            .map(|rest| rest.replacen(':', "/", 1))
            .unwrap_or_else(|| trimmed.to_string()),
    };
    let normalized = without_scheme.map(str::to_string).unwrap_or(pathish);
    let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    let host = parts.first()?.to_string();
    let owner = parts.get(parts.len().checked_sub(2)?)?.to_string();
    let repo = parts.last()?.to_string();
    Some(RemoteInfo { host, owner, repo })
}

// =========================== Forge auth =====================================

pub struct ForgeAuthProbe {
    pub forge: ForgeCli,
}

impl Job for ForgeAuthProbe {
    fn name(&self) -> &'static str {
        "probe.forge.auth"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        JobOutcome::Done(Box::new(self.forge.auth_status()))
    }
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
        let result = match process::jj_output(
            &self.jj_binary,
            &["log", "-r", &self.revset, "--no-graph", "-T", TEMPLATE],
        ) {
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
        //
        // `--ignore-working-copy`: this probe is dispatched only after
        // `RevsetProbe` completes (see `App::absorb_payload`), which already
        // snapshotted `@`. Skipping a second snapshot here avoids redundant
        // work and the repo write-lock it would take.
        const TEMPLATE: &str = r#""\x1E" ++ commit_id.short() ++ "|" ++ change_id.short() ++ "|" ++ bookmarks.map(|b| b.name()).join(",") ++ "|" ++ description.lines().join("\x1F") ++ "\x1D\n""#;
        let result = match process::jj(
            &self.jj_binary,
            &[
                "--ignore-working-copy",
                "log",
                "-r",
                &self.revset,
                "--no-graph",
                "--stat",
                "-T",
                TEMPLATE,
            ],
        ) {
            Ok(stdout) => {
                let pairs = parse_revset_log_with_stats(&stdout)
                    .into_iter()
                    .map(|s| (s.change_id, s.stats))
                    .collect();
                RevsetStats(pairs)
            }
            // Failures fall through silently — first paint already
            // succeeded; missing stats just leave rows without the
            // compact "4f +188 -34" metadata line.
            Err(_) => RevsetStats::default(),
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
        JobOutcome::Done(Box::new(fetch_base_bookmarks(&self.jj_binary)))
    }
}

fn fetch_base_bookmarks(jj_binary: &str) -> BaseBookmarks {
    // `--ignore-working-copy`: bookmark listing reads only repo refs and
    // never depends on the working-copy snapshot, so we skip taking one.
    const TEMPLATE: &str = r#"name() ++ "\t" ++ remote().name() ++ "\n""#;
    match process::jj(
        jj_binary,
        &[
            "--ignore-working-copy",
            "bookmark",
            "list",
            "--all-remotes",
            "-T",
            TEMPLATE,
        ],
    ) {
        Ok(stdout) => parse_base_bookmarks(&stdout),
        Err(_) => Vec::new(),
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

// ======================== Existing stack PRs ================================

pub struct StackExistingPrsProbe {
    pub forge: ForgeCli,
    pub owner: Option<String>,
    pub repo: Option<String>,
}

impl Job for StackExistingPrsProbe {
    fn name(&self) -> &'static str {
        "probe.stack_existing_prs"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        JobOutcome::Done(Box::new(fetch_existing_prs(
            &self.forge,
            self.owner.as_deref(),
            self.repo.as_deref(),
        )))
    }
}

fn fetch_existing_prs(
    forge: &ForgeCli,
    owner: Option<&str>,
    repo: Option<&str>,
) -> StackExistingPrs {
    forge.existing_prs(owner, repo)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StackPushPrecheck {
    pub bookmarks: BaseBookmarks,
    pub existing_prs: StackExistingPrs,
}

pub struct StackPushPrecheckJob {
    pub jj_binary: String,
    pub forge: ForgeCli,
    pub owner: Option<String>,
    pub repo: Option<String>,
}

impl Job for StackPushPrecheckJob {
    fn name(&self) -> &'static str {
        "probe.stack_push_precheck"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        JobOutcome::Done(Box::new(StackPushPrecheck {
            bookmarks: fetch_base_bookmarks(&self.jj_binary),
            existing_prs: fetch_existing_prs(
                &self.forge,
                self.owner.as_deref(),
                self.repo.as_deref(),
            ),
        }))
    }
}

// ======================== Repo options =====================================

pub struct RepoOptionsProbe {
    pub forge: ForgeCli,
    pub owner: String,
    pub repo: String,
}

impl Job for RepoOptionsProbe {
    fn name(&self) -> &'static str {
        "probe.repo_options"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        JobOutcome::Done(Box::new(self.forge.repo_options(&self.owner, &self.repo)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Job;

    const MISSING_BINARY: &str = "__teatui_missing_probe_binary__";

    fn run_job<T: Send + 'static, J: Job>(job: J) -> T {
        match Box::new(job).run() {
            JobOutcome::Done(payload) => *payload.downcast::<T>().expect("payload type"),
            JobOutcome::Failed(message) => panic!("job failed: {message}"),
        }
    }

    #[test]
    fn version_probe_reports_missing_binary() {
        assert_eq!(run_version_check(MISSING_BINARY), ToolStatus::Missing);
    }

    #[test]
    fn workspace_probe_missing_jj_is_errored_not_outside() {
        let workspace: WorkspaceInfo = run_job(WorkspaceProbe {
            jj_binary: MISSING_BINARY.into(),
        });

        assert_eq!(
            workspace,
            WorkspaceInfo::Errored {
                message: format!("{MISSING_BINARY} not found"),
            }
        );
    }

    #[test]
    fn parses_https_remote_info() {
        assert_eq!(
            parse_remote_info("https://gitea.example.com/owner/repo.git"),
            Some(RemoteInfo {
                host: "gitea.example.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn parses_scp_like_ssh_remote_info() {
        assert_eq!(
            parse_remote_info("git@gitea.example.com:owner/repo.git"),
            Some(RemoteInfo {
                host: "gitea.example.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn parses_ssh_scheme_remote_info() {
        assert_eq!(
            parse_remote_info("ssh://git@gitea.example.com/owner/repo.git"),
            Some(RemoteInfo {
                host: "gitea.example.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn extracts_origin_remote_url_from_git_config() {
        let raw = r#"
[core]
    repositoryformatversion = 0
[remote "upstream"]
    url = https://gitea.example.com/owner/repo.git
[remote "origin"]
    url = https://github.com/Noghpu/teatui.git
    fetch = +refs/heads/*:refs/remotes/origin/*
"#;
        assert_eq!(
            remote_url_from_git_config(raw),
            Some(("origin".to_string(), "https://github.com/Noghpu/teatui.git".to_string()))
        );
    }

    #[test]
    fn falls_back_to_first_parseable_git_config_remote_without_origin() {
        let raw = r#"
[remote "fork"]
    url = git@github.com:Noghpu/teatui.git
"#;
        assert_eq!(
            remote_url_from_git_config(raw),
            Some(("fork".to_string(), "git@github.com:Noghpu/teatui.git".to_string()))
        );
    }

    #[test]
    fn reads_colocated_git_config_from_jj_git_target() {
        let root =
            std::env::temp_dir().join(format!("teatui-jj-git-target-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".jj/repo/store")).expect("create jj store");
        std::fs::create_dir_all(root.join(".git")).expect("create git dir");
        std::fs::write(root.join(".jj/repo/store/git_target"), "../../../.git")
            .expect("write git_target");
        std::fs::write(
            root.join(".git/config"),
            r#"[remote "origin"]
    url = https://github.com/Noghpu/teatui.git
"#,
        )
        .expect("write git config");

        assert_eq!(
            remote_url_from_colocated_git_config(&root),
            Some(("origin".to_string(), "https://github.com/Noghpu/teatui.git".to_string()))
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn revset_stats_probe_falls_back_on_missing_jj() {
        let stats: RevsetStats = run_job(RevsetStatsProbe {
            jj_binary: MISSING_BINARY.into(),
            revset: "trunk()..@".into(),
        });

        assert_eq!(stats, RevsetStats::default());
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
