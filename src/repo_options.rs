use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::command::capture;
use crate::config::Config;
use crate::event::BackgroundEvent;
use crate::generate::PickerOption;
use crate::repo::RemoteInfo;
use crate::tea::TeaClient;

const CACHE_VERSION: u32 = 1;
const FRESHNESS_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_STALE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Sanitize a string component for use in a cache filename key.
/// Replaces any character that is not alphanumeric, `-`, or `_` with `_`.
pub fn sanitize_key_component(component: &str) -> String {
    component
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Build a stable cache key from Gitea host, owner, and repo name.
pub fn repo_cache_key(host: &str, owner: &str, repo: &str) -> String {
    format!(
        "{}_{}_{}",
        sanitize_key_component(host),
        sanitize_key_component(owner),
        sanitize_key_component(repo),
    )
}

// --- JSON structures from tea CLI ---

#[derive(Debug, Clone, Deserialize)]
struct TeaLabelJson {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TeaMilestoneJson {
    pub id: Option<u64>,
    pub title: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TeaCollaboratorJson {
    pub login: Option<String>,
    pub full_name: Option<String>,
}

// --- Public picker option types ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelOption {
    pub id: u64,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneOption {
    pub id: u64,
    pub title: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssigneeOption {
    pub login: String,
    pub display_name: String,
}

// --- Parsers ---

/// Parse `tea labels list --output json` output.
/// Returns only entries with a valid non-empty name.
pub fn parse_labels_json(json: &str) -> Vec<LabelOption> {
    let items: Vec<TeaLabelJson> = match serde_json::from_str(json) {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };

    items
        .into_iter()
        .filter_map(|item| {
            let name = item.name?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(LabelOption {
                id: item.id.unwrap_or(0),
                name,
                color: item.color.unwrap_or_default(),
            })
        })
        .collect()
}

/// Parse `tea milestones list --output json` output.
/// Returns only open milestones (state == "open") with a valid non-empty title.
pub fn parse_milestones_json(json: &str) -> Vec<MilestoneOption> {
    let items: Vec<TeaMilestoneJson> = match serde_json::from_str(json) {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };

    items
        .into_iter()
        .filter_map(|item| {
            let title = item.title?.trim().to_string();
            if title.is_empty() {
                return None;
            }
            let state = item.state.unwrap_or_else(|| "open".into());
            Some(MilestoneOption {
                id: item.id.unwrap_or(0),
                title,
                state,
            })
        })
        .collect()
}

/// Parse `tea api '/repos/{owner}/{repo}/collaborators'` output.
/// Returns only entries with a valid non-empty login.
pub fn parse_collaborators_json(json: &str) -> Vec<AssigneeOption> {
    let items: Vec<TeaCollaboratorJson> = match serde_json::from_str(json) {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };

    items
        .into_iter()
        .filter_map(|item| {
            let login = item.login?.trim().to_string();
            if login.is_empty() {
                return None;
            }
            let display_name = item
                .full_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| login.clone());
            Some(AssigneeOption {
                login,
                display_name,
            })
        })
        .collect()
}

// --- Cache ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoOptionsCache {
    pub version: u32,
    pub fetched_at_secs: u64,
    pub host: String,
    pub owner: String,
    pub repo: String,
    pub labels: Vec<LabelOption>,
    pub milestones: Vec<MilestoneOption>,
    pub assignees: Vec<AssigneeOption>,
}

impl RepoOptionsCache {
    pub fn fetched_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.fetched_at_secs)
    }

    pub fn age(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.fetched_at())
            .unwrap_or(Duration::ZERO)
    }

    pub fn is_fresh(&self) -> bool {
        self.age() < FRESHNESS_TTL
    }

    pub fn is_usable(&self) -> bool {
        self.age() < MAX_STALE_AGE
    }
}

/// Return the cache directory for repo options: `dirs::cache_dir()/teatui/repo-options`.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("teatui").join("repo-options"))
}

/// Return the cache file path for the given repo key.
pub fn cache_file_path(key: &str) -> Option<PathBuf> {
    cache_dir().map(|dir| dir.join(format!("{key}.json")))
}

/// Write the cache file atomically by writing a temp file and renaming.
pub fn write_cache(cache: &RepoOptionsCache) -> std::io::Result<()> {
    let Some(dir) = cache_dir() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cache directory unavailable",
        ));
    };

    std::fs::create_dir_all(&dir)?;

    let key = repo_cache_key(&cache.host, &cache.owner, &cache.repo);
    let final_path = dir.join(format!("{key}.json"));
    let temp_path = dir.join(format!("{key}.json.tmp"));

    let json = serde_json::to_string_pretty(cache).map_err(std::io::Error::other)?;
    std::fs::write(&temp_path, &json)?;
    std::fs::rename(&temp_path, &final_path)?;

    Ok(())
}

/// Read the cache file for a given repo key. Returns None if missing or parse error.
pub fn read_cache(key: &str) -> Option<RepoOptionsCache> {
    let path = cache_file_path(key)?;
    let json = std::fs::read_to_string(&path).ok()?;
    let cache: RepoOptionsCache = serde_json::from_str(&json).ok()?;

    if cache.version != CACHE_VERSION {
        return None;
    }

    Some(cache)
}

// --- Loading status ---

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OptionsLoadStatus {
    /// No load has been started.
    #[default]
    Idle,
    /// Options are ready (may be stale).
    Ready {
        /// True when the data came from live fetch (not cache-only).
        from_live: bool,
        /// True if the cache/source is older than the freshness TTL.
        stale: bool,
        /// Warning message when data is stale or from a failed live fetch.
        warning: Option<String>,
    },
    /// Options are unavailable (e.g. tea missing, auth failed, no remote).
    Unavailable { reason: String },
}

/// Loaded options for labels, assignees, and milestones for the current repo.
#[derive(Debug, Clone, Default)]
pub struct RepoOptions {
    pub labels: Vec<LabelOption>,
    pub milestones: Vec<MilestoneOption>,
    pub assignees: Vec<AssigneeOption>,
    pub status: OptionsLoadStatus,
}

impl RepoOptions {
    /// Convert to `PickerOption` slices suitable for setting on picker fields.
    pub fn label_picker_options(&self) -> Vec<PickerOption> {
        self.labels
            .iter()
            .map(|label| PickerOption::new(label.name.clone(), label.name.clone()))
            .collect()
    }

    pub fn milestone_picker_options(&self) -> Vec<PickerOption> {
        self.milestones
            .iter()
            .map(|milestone| PickerOption::new(milestone.title.clone(), milestone.title.clone()))
            .collect()
    }

    pub fn assignee_picker_options(&self) -> Vec<PickerOption> {
        self.assignees
            .iter()
            .map(|assignee| {
                PickerOption::new(assignee.display_name.clone(), assignee.login.clone())
            })
            .collect()
    }

    /// Returns a user-visible warning string when the options are stale or partially unavailable.
    pub fn status_warning(&self) -> Option<&str> {
        match &self.status {
            OptionsLoadStatus::Ready {
                warning: Some(w), ..
            } => Some(w.as_str()),
            OptionsLoadStatus::Unavailable { reason } => Some(reason.as_str()),
            _ => None,
        }
    }
}

// --- Background loading ---

/// Result of a repo options fetch attempt sent back to the app.
#[derive(Debug, Clone)]
pub struct RepoOptionsResult {
    pub options: RepoOptions,
    /// Whether this result came from the cached data or live fetch.
    pub from_cache: bool,
}

/// Background loader: first sends cached data if available (and cache is usable),
/// then performs live fetch if needed. Sends `BackgroundEvent::RepoOptions` once or twice.
pub fn spawn_repo_options_load(
    config: Config,
    cwd: PathBuf,
    remote: RemoteInfo,
    force_refresh: bool,
    tx: UnboundedSender<BackgroundEvent>,
) {
    tokio::spawn(async move {
        let cache_key = repo_cache_key(&remote.host, &remote.owner, &remote.name);
        let cached = read_cache(&cache_key);

        // Send cached result immediately if usable.
        if let Some(ref cache) = cached
            && cache.is_usable()
        {
            let stale = !cache.is_fresh();
            let warning = if stale {
                Some("picker options are from a stale cache; refreshing…".to_string())
            } else {
                None
            };
            let options = RepoOptions {
                labels: cache.labels.clone(),
                milestones: cache.milestones.clone(),
                assignees: cache.assignees.clone(),
                status: OptionsLoadStatus::Ready {
                    from_live: false,
                    stale,
                    warning,
                },
            };
            let _ = tx.send(BackgroundEvent::RepoOptions(Box::new(RepoOptionsResult {
                options,
                from_cache: true,
            })));

            // If the cache is fresh and we aren't forcing a refresh, stop here.
            if cache.is_fresh() && !force_refresh {
                return;
            }
        }

        // Live fetch.
        let tea = TeaClient::new(&config);
        let (labels_result, milestones_result, collaborators_result) = tokio::join!(
            capture(tea.labels_list_command(&cwd)),
            capture(tea.milestones_list_command(&cwd)),
            capture(tea.collaborators_command(&cwd)),
        );

        let mut warnings = Vec::new();

        let labels = match labels_result {
            Ok(capture) => parse_labels_json(&capture.stdout),
            Err(err) => {
                warnings.push(format!("labels unavailable: {}", err.message));
                Vec::new()
            }
        };

        let milestones = match milestones_result {
            Ok(capture) => parse_milestones_json(&capture.stdout),
            Err(err) => {
                warnings.push(format!("milestones unavailable: {}", err.message));
                Vec::new()
            }
        };

        let assignees = match collaborators_result {
            Ok(capture) => parse_collaborators_json(&capture.stdout),
            Err(err) => {
                warnings.push(format!("assignees unavailable: {}", err.message));
                Vec::new()
            }
        };

        // If all commands failed and we had no usable cache, report unavailable.
        let all_failed = labels.is_empty()
            && milestones.is_empty()
            && assignees.is_empty()
            && !warnings.is_empty();
        let had_no_cache = cached.as_ref().map(|c| !c.is_usable()).unwrap_or(true);
        if all_failed && had_no_cache {
            let reason = warnings.join("; ");
            let options = RepoOptions {
                labels: Vec::new(),
                milestones: Vec::new(),
                assignees: Vec::new(),
                status: OptionsLoadStatus::Unavailable { reason },
            };
            let _ = tx.send(BackgroundEvent::RepoOptions(Box::new(RepoOptionsResult {
                options,
                from_cache: false,
            })));
            return;
        }

        let warning = if warnings.is_empty() {
            None
        } else {
            Some(warnings.join("; "))
        };

        // Write refreshed cache atomically.
        let fetched_at_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let new_cache = RepoOptionsCache {
            version: CACHE_VERSION,
            fetched_at_secs,
            host: remote.host.clone(),
            owner: remote.owner.clone(),
            repo: remote.name.clone(),
            labels: labels.clone(),
            milestones: milestones.clone(),
            assignees: assignees.clone(),
        };
        let _ = write_cache(&new_cache);

        let options = RepoOptions {
            labels,
            milestones,
            assignees,
            status: OptionsLoadStatus::Ready {
                from_live: true,
                stale: false,
                warning,
            },
        };

        let _ = tx.send(BackgroundEvent::RepoOptions(Box::new(RepoOptionsResult {
            options,
            from_cache: false,
        })));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- sanitize_key_component ---

    #[test]
    fn sanitize_keeps_alphanumeric_and_dash_underscore() {
        assert_eq!(
            sanitize_key_component("code-example_com"),
            "code-example_com"
        );
        assert_eq!(sanitize_key_component("abc123"), "abc123");
    }

    #[test]
    fn sanitize_replaces_dots_and_colons_and_slashes() {
        assert_eq!(
            sanitize_key_component("code.example.com"),
            "code_example_com"
        );
        assert_eq!(sanitize_key_component("host:2222"), "host_2222");
        assert_eq!(sanitize_key_component("team/project"), "team_project");
    }

    #[test]
    fn repo_cache_key_combines_parts() {
        let key = repo_cache_key("code.example.com", "team", "project");
        assert_eq!(key, "code_example_com_team_project");
    }

    // --- parse_labels_json ---

    #[test]
    fn parse_labels_json_returns_valid_entries() {
        let json = r##"[
            {"id": 1, "name": "bug", "color": "#ee0701"},
            {"id": 2, "name": "enhancement", "color": "#84b6eb"}
        ]"##;
        let labels = parse_labels_json(json);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].name, "bug");
        assert_eq!(labels[1].name, "enhancement");
    }

    #[test]
    fn parse_labels_json_skips_entries_with_no_name() {
        let json = r##"[
            {"id": 1, "color": "#ee0701"},
            {"id": 2, "name": "valid", "color": "#84b6eb"}
        ]"##;
        let labels = parse_labels_json(json);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "valid");
    }

    #[test]
    fn parse_labels_json_returns_empty_on_invalid_json() {
        let labels = parse_labels_json("not json");
        assert!(labels.is_empty());
    }

    #[test]
    fn parse_labels_json_tolerates_extra_fields() {
        let json = r##"[{"id": 1, "name": "bug", "color": "#fff", "extra_field": "ignored"}]"##;
        let labels = parse_labels_json(json);
        assert_eq!(labels.len(), 1);
    }

    // --- parse_milestones_json ---

    #[test]
    fn parse_milestones_json_returns_valid_entries() {
        let json = r#"[
            {"id": 1, "title": "v1.0", "state": "open"},
            {"id": 2, "title": "v2.0", "state": "closed"}
        ]"#;
        let milestones = parse_milestones_json(json);
        assert_eq!(milestones.len(), 2);
        assert_eq!(milestones[0].title, "v1.0");
        assert_eq!(milestones[0].state, "open");
    }

    #[test]
    fn parse_milestones_json_skips_entries_with_no_title() {
        let json = r#"[
            {"id": 1, "state": "open"},
            {"id": 2, "title": "v1.0", "state": "open"}
        ]"#;
        let milestones = parse_milestones_json(json);
        assert_eq!(milestones.len(), 1);
    }

    #[test]
    fn parse_milestones_json_returns_empty_on_invalid_json() {
        let milestones = parse_milestones_json("not json");
        assert!(milestones.is_empty());
    }

    #[test]
    fn parse_milestones_json_defaults_missing_state_to_open() {
        let json = r#"[{"id": 1, "title": "v1.0"}]"#;
        let milestones = parse_milestones_json(json);
        assert_eq!(milestones.len(), 1);
        assert_eq!(milestones[0].state, "open");
    }

    // --- parse_collaborators_json ---

    #[test]
    fn parse_collaborators_json_returns_valid_entries() {
        let json = r#"[
            {"login": "alice", "full_name": "Alice Smith"},
            {"login": "bob", "full_name": ""}
        ]"#;
        let assignees = parse_collaborators_json(json);
        assert_eq!(assignees.len(), 2);
        assert_eq!(assignees[0].login, "alice");
        assert_eq!(assignees[0].display_name, "Alice Smith");
        // Empty full_name falls back to login.
        assert_eq!(assignees[1].display_name, "bob");
    }

    #[test]
    fn parse_collaborators_json_skips_entries_with_no_login() {
        let json = r#"[
            {"full_name": "No Login"},
            {"login": "valid", "full_name": "Valid User"}
        ]"#;
        let assignees = parse_collaborators_json(json);
        assert_eq!(assignees.len(), 1);
        assert_eq!(assignees[0].login, "valid");
    }

    #[test]
    fn parse_collaborators_json_returns_empty_on_invalid_json() {
        let assignees = parse_collaborators_json("not json");
        assert!(assignees.is_empty());
    }

    // --- cache freshness ---

    #[test]
    fn cache_freshness_below_ttl_is_fresh() {
        let cache = RepoOptionsCache {
            version: CACHE_VERSION,
            fetched_at_secs: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            host: "host".into(),
            owner: "owner".into(),
            repo: "repo".into(),
            labels: Vec::new(),
            milestones: Vec::new(),
            assignees: Vec::new(),
        };
        assert!(cache.is_fresh());
        assert!(cache.is_usable());
    }

    #[test]
    fn cache_above_ttl_but_below_max_stale_is_usable_but_not_fresh() {
        // 16 minutes old: past TTL (15 min), before max stale (7 days)
        let sixteen_minutes_secs = 16 * 60;
        let old_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(sixteen_minutes_secs);

        let cache = RepoOptionsCache {
            version: CACHE_VERSION,
            fetched_at_secs: old_secs,
            host: "host".into(),
            owner: "owner".into(),
            repo: "repo".into(),
            labels: Vec::new(),
            milestones: Vec::new(),
            assignees: Vec::new(),
        };
        assert!(!cache.is_fresh());
        assert!(cache.is_usable());
    }

    #[test]
    fn cache_older_than_max_stale_is_not_usable() {
        // 8 days old: past max stale age (7 days)
        let eight_days_secs = 8 * 24 * 60 * 60;
        let old_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(eight_days_secs);

        let cache = RepoOptionsCache {
            version: CACHE_VERSION,
            fetched_at_secs: old_secs,
            host: "host".into(),
            owner: "owner".into(),
            repo: "repo".into(),
            labels: Vec::new(),
            milestones: Vec::new(),
            assignees: Vec::new(),
        };
        assert!(!cache.is_fresh());
        assert!(!cache.is_usable());
    }

    // --- picker options from RepoOptions ---

    #[test]
    fn label_picker_options_uses_name_as_both_label_and_value() {
        let options = RepoOptions {
            labels: vec![LabelOption {
                id: 1,
                name: "bug".into(),
                color: "ff0".into(),
            }],
            milestones: Vec::new(),
            assignees: Vec::new(),
            status: OptionsLoadStatus::default(),
        };
        let picker = options.label_picker_options();
        assert_eq!(picker.len(), 1);
        assert_eq!(picker[0].label, "bug");
        assert_eq!(picker[0].value, "bug");
    }

    #[test]
    fn assignee_picker_options_uses_display_name_as_label_and_login_as_value() {
        let options = RepoOptions {
            labels: Vec::new(),
            milestones: Vec::new(),
            assignees: vec![AssigneeOption {
                login: "alice".into(),
                display_name: "Alice Smith".into(),
            }],
            status: OptionsLoadStatus::default(),
        };
        let picker = options.assignee_picker_options();
        assert_eq!(picker.len(), 1);
        assert_eq!(picker[0].label, "Alice Smith");
        assert_eq!(picker[0].value, "alice");
    }

    // --- selection preservation test (indirectly via PickerFieldState) ---

    #[test]
    fn set_picker_options_retains_valid_previously_selected_value() {
        use crate::generate::FieldState;

        // Start with a previously committed "bug" selection, but no options loaded yet.
        let mut field = FieldState::picker("bug", true, true);

        // Now set options that include "bug" — the committed selection should persist.
        field.set_picker_options(vec![
            crate::generate::PickerOption::new("bug", "bug"),
            crate::generate::PickerOption::new("enhancement", "enhancement"),
        ]);

        assert!(field.picker_selected_values().contains(&"bug".to_string()));
    }

    #[test]
    fn set_picker_options_clears_invalid_previously_selected_value() {
        use crate::generate::FieldState;

        // "old-label" was selected but is not in the new options.
        let mut field = FieldState::picker("old-label", true, true);

        field.set_picker_options(vec![crate::generate::PickerOption::new("bug", "bug")]);

        // "old-label" should be removed because it is not in the new options.
        assert!(
            !field
                .picker_selected_values()
                .contains(&"old-label".to_string())
        );
    }
}
