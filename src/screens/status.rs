//! Shared formatters that turn `Cached<T>` values from `StatusStore` into
//! short status strings for display. Used by the landing pane and the
//! Generate sidebar.

use crate::domain::{LlmHealth, Revsets, TeaAuthStatus, ToolStatus, WorkspaceInfo};
use crate::runtime::Cached;

pub fn render_cached<T, F: Fn(&T) -> String>(c: &Cached<T>, fmt: F) -> String {
    match c {
        Cached::Unknown => "·".into(),
        Cached::Loading => "loading…".into(),
        Cached::Ready(v) => fmt(v),
        Cached::Stale { value, refreshing } => {
            let v = fmt(value);
            if *refreshing {
                format!("{v} (refreshing…)")
            } else {
                v
            }
        }
    }
}

pub fn render_tool(c: &Cached<ToolStatus>) -> String {
    render_cached(c, |s| match s {
        ToolStatus::Available { version } => version.clone(),
        ToolStatus::Missing => "missing".to_string(),
        ToolStatus::Errored { message } => format!("error: {message}"),
    })
}

pub fn render_workspace(c: &Cached<WorkspaceInfo>) -> String {
    render_cached(c, |w| match w {
        WorkspaceInfo::Inside { root, .. } => format!("inside {}", root.display()),
        WorkspaceInfo::Outside => "outside any jj workspace".to_string(),
        WorkspaceInfo::Errored { message } => format!("error: {message}"),
    })
}

pub fn render_auth(c: &Cached<TeaAuthStatus>) -> String {
    render_cached(c, |a| match a {
        TeaAuthStatus::Configured { logins } => format!("configured ({})", logins.join(", ")),
        TeaAuthStatus::None => "no logins".to_string(),
        TeaAuthStatus::Errored { message } => format!("error: {message}"),
    })
}

pub fn render_llm(c: &Cached<LlmHealth>) -> String {
    render_cached(c, |h| match h {
        LlmHealth::Available { models } => {
            if models.is_empty() {
                "reachable (no models)".to_string()
            } else {
                format!("reachable ({} models)", models.len())
            }
        }
        LlmHealth::Unreachable { message } => format!("unreachable: {message}"),
    })
}

pub fn render_revsets(c: &Cached<Revsets>) -> String {
    render_cached(c, |r| match r {
        Revsets::Loaded(items) => format!("{} change(s)", items.len()),
        Revsets::Errored { message } => format!("error: {message}"),
    })
}
