use crate::runtime::Cached;

use super::probe::{
    BaseBookmarks, LlmHealth, RepoOptions, Revsets, TeaAuthStatus, ToolStatus, VersionKind,
    VersionResult, WorkspaceInfo,
};

#[derive(Debug, Clone, Default)]
pub struct StatusStore {
    pub jj: Cached<ToolStatus>,
    pub git: Cached<ToolStatus>,
    pub tea: Cached<ToolStatus>,
    pub workspace: Cached<WorkspaceInfo>,
    pub tea_auth: Cached<TeaAuthStatus>,
    pub llm: Cached<LlmHealth>,
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

    pub fn set_revsets(&mut self, revsets: Revsets) {
        self.revsets.set(revsets);
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
}
