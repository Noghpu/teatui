use std::collections::HashMap;

use crate::runtime::Cached;

use super::probe::{
    BaseBookmarks, LlmHealth, RepoOptions, RevsetStats, Revsets, TeaAuthStatus, ToolStatus,
    VersionKind, VersionResult, WorkspaceInfo,
};

#[derive(Debug, Clone, Default)]
pub struct StatusStore {
    pub jj: Cached<ToolStatus>,
    pub git: Cached<ToolStatus>,
    pub tea: Cached<ToolStatus>,
    pub workspace: Cached<WorkspaceInfo>,
    pub tea_auth: Cached<TeaAuthStatus>,
    /// Health of the *active* backend — drives the landing LLM chip.
    pub llm: Cached<LlmHealth>,
    /// Health of every configured backend, keyed by name. Populated lazily
    /// by the backend switcher, which probes all backends when opened.
    pub backend_health: HashMap<String, Cached<LlmHealth>>,
    pub revsets: Cached<Revsets>,
    pub base_bookmarks: Cached<BaseBookmarks>,
    pub repo_options: Cached<RepoOptions>,
}

impl StatusStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark every probe-driven field as in-flight. Called immediately
    /// before submitting boot probes so views render the right
    /// (Loading / Stale-refreshing) state.
    pub fn mark_all_loading(&mut self) {
        self.jj.mark_loading();
        self.git.mark_loading();
        self.tea.mark_loading();
        self.workspace.mark_loading();
        self.tea_auth.mark_loading();
        self.llm.mark_loading();
        self.revsets.mark_loading();
        self.base_bookmarks.mark_loading();
    }

    pub fn set_version(&mut self, result: VersionResult) {
        let slot = match result.kind {
            VersionKind::Jj => &mut self.jj,
            VersionKind::Git => &mut self.git,
            VersionKind::Tea => &mut self.tea,
        };
        slot.set(result.status);
    }

    pub fn set_workspace(&mut self, info: WorkspaceInfo) {
        self.workspace.set(info);
    }

    pub fn set_tea_auth(&mut self, status: TeaAuthStatus) {
        self.tea_auth.set(status);
    }

    pub fn set_llm(&mut self, health: LlmHealth) {
        self.llm.set(health);
    }

    /// Mark a backend's health as in-flight before its probe is submitted.
    /// Keeps any prior value as `Stale { refreshing }` so a known-good row
    /// keeps its symbol while re-probing, rather than flashing to pending.
    pub fn mark_backend_loading(&mut self, name: &str) {
        self.backend_health
            .entry(name.to_string())
            .or_default()
            .mark_loading();
    }

    pub fn set_backend_health(&mut self, name: String, health: LlmHealth) {
        self.backend_health.entry(name).or_default().set(health);
    }

    /// Cached health for a single backend, if it has ever been probed.
    /// `None` means "never probed" — the switcher renders that as pending.
    pub fn backend_health(&self, name: &str) -> Option<&Cached<LlmHealth>> {
        self.backend_health.get(name)
    }

    pub fn set_revsets(&mut self, revsets: Revsets) {
        self.revsets.set(revsets);
    }

    /// Merge deferred diff-stat results into the existing revset list.
    /// Stats are matched by `change_id`; rows that are no longer in the
    /// list (e.g. revset changed between probe and stats fetch) are
    /// silently dropped.
    pub fn merge_revset_stats(&mut self, stats: RevsetStats) {
        let lookup: std::collections::HashMap<&str, &str> = stats
            .0
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let Some(Revsets::Loaded(items)) = self.revsets.value_mut() else {
            return;
        };
        for item in items.iter_mut() {
            if let Some(s) = lookup.get(item.change_id.as_str()) {
                item.stats = (*s).to_string();
            }
        }
    }

    pub fn set_base_bookmarks(&mut self, bookmarks: BaseBookmarks) {
        self.base_bookmarks.set(bookmarks);
    }

    pub fn mark_repo_options_loading(&mut self) {
        self.repo_options.mark_loading();
    }

    pub fn set_repo_options(&mut self, options: RepoOptions) {
        self.repo_options.set(options);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::probe::ToolStatus;

    #[test]
    fn mark_all_loading_then_set_version_replaces_loading() {
        let mut store = StatusStore::new();
        store.mark_all_loading();
        assert!(matches!(store.jj, Cached::Loading));
        store.set_version(VersionResult {
            kind: VersionKind::Jj,
            status: ToolStatus::Available {
                version: "jj 0.30".into(),
            },
        });
        assert!(matches!(store.jj, Cached::Ready(_)));
        assert!(matches!(store.git, Cached::Loading));
    }

    #[test]
    fn mark_all_loading_includes_revsets() {
        let mut store = StatusStore::new();
        store.mark_all_loading();
        assert!(matches!(store.revsets, Cached::Loading));
    }

    #[test]
    fn merge_revset_stats_fills_matching_change_ids_only() {
        use crate::domain::probe::{RevsetStats, RevsetSummary, Revsets};
        let mut store = StatusStore::new();
        store.set_revsets(Revsets::Loaded(vec![
            RevsetSummary {
                label: "trunk()..aaaa".into(),
                change_id: "aaaa".into(),
                commit_id: "1111".into(),
                bookmarks: vec![],
                description: "first".into(),
                description_body: String::new(),
                author: String::new(),
                stats: String::new(),
                commit_count: 1,
                commit_ids: vec!["1111".into()],
                change_ids: vec!["aaaa".into()],
                recent_log: vec![],
                warnings: vec![],
            },
            RevsetSummary {
                label: "trunk()..bbbb".into(),
                change_id: "bbbb".into(),
                commit_id: "2222".into(),
                bookmarks: vec![],
                description: "second".into(),
                description_body: String::new(),
                author: String::new(),
                stats: String::new(),
                commit_count: 1,
                commit_ids: vec!["2222".into()],
                change_ids: vec!["bbbb".into()],
                recent_log: vec![],
                warnings: vec![],
            },
        ]));
        store.merge_revset_stats(RevsetStats(vec![
            ("aaaa".into(), "1 file changed, 5 insertions(+)".into()),
            ("cccc".into(), "should be ignored — no matching row".into()),
        ]));
        let items = match store.revsets.value() {
            Some(Revsets::Loaded(v)) => v,
            _ => panic!("expected loaded revsets"),
        };
        assert_eq!(items[0].stats, "1 file changed, 5 insertions(+)");
        assert_eq!(items[1].stats, "", "rows without matching stats stay empty");
    }
}
