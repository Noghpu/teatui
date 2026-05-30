//! Render-path smoke tests. The contract is: every screen, in every phase
//! we ship, must render against a `TestBackend` without panicking. These
//! tests are deliberately not assertions on exact output — they catch
//! layout / widget construction errors and overflows.

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use teatui::domain::{
    ContextBundle, GeneratedDraft, LlmHealth, PromptBuild, PromptManifest, PromptSection,
    RevsetSummary, Revsets, StatusStore, TeaAuthStatus, ToolStatus, VersionKind, VersionResult,
    WorkspaceInfo, build_prompt,
};
use teatui::runtime::Cached;
use teatui::screens::generate::{GeneratePhase, GenerateState, Pane};
use teatui::screens::{self, LandingState};

fn term() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(120, 30)).expect("terminal")
}

fn draw_landing(state: &LandingState, status: &StatusStore) {
    let mut t = term();
    t.draw(|frame| {
        let area = frame.area();
        screens::landing::render(state, status, frame, area);
    })
    .expect("draw");
}

fn draw_generate(state: &GenerateState, status: &StatusStore) {
    let mut t = term();
    t.draw(|frame| {
        let area = frame.area();
        screens::generate::render(state, status, frame, area);
    })
    .expect("draw");
}

fn populated_status() -> StatusStore {
    let mut s = StatusStore::new();
    s.set_version(VersionResult {
        kind: VersionKind::Jj,
        status: ToolStatus::Available {
            version: "jj 0.30.0".into(),
        },
    });
    s.set_version(VersionResult {
        kind: VersionKind::Git,
        status: ToolStatus::Available {
            version: "git 2.42.0".into(),
        },
    });
    s.set_version(VersionResult {
        kind: VersionKind::Tea,
        status: ToolStatus::Available {
            version: "tea 0.10.0".into(),
        },
    });
    s.set_workspace(WorkspaceInfo::Inside {
        root: PathBuf::from("/home/user/proj"),
    });
    s.set_tea_auth(TeaAuthStatus::Configured {
        logins: vec!["gitea".into()],
    });
    s.set_llm(LlmHealth::Available {
        models: vec!["llama3".into(), "qwen2.5-coder".into()],
    });
    s.set_revsets(Revsets::Loaded(vec![sample_revset("abcd", "Add foo")]));
    s
}

fn sample_revset(change_id: &str, desc: &str) -> RevsetSummary {
    RevsetSummary {
        change_id: change_id.into(),
        commit_id: "deadbeef".into(),
        bookmarks: vec![],
        description: desc.into(),
        author: "alice".into(),
    }
}

fn sample_prompt() -> PromptBuild {
    PromptBuild {
        prompt: "PROMPT BODY".into(),
        manifest: PromptManifest {
            sections: vec![
                PromptSection {
                    name: "Status",
                    bytes: 12,
                },
                PromptSection {
                    name: "Diff",
                    bytes: 4096,
                },
            ],
            total_bytes: 4200,
        },
    }
}

fn sample_draft() -> GeneratedDraft {
    GeneratedDraft {
        title: "Add foo to bar".into(),
        description: "Implements foo.\n\nDetails follow.".into(),
    }
}

fn sample_context() -> ContextBundle {
    ContextBundle {
        revset: "abcd".into(),
        status: "Working copy : abcd".into(),
        log: "abcd Test".into(),
        diff_stats: "1 file changed".into(),
        diff: "@@ -1 +1 @@\n-old\n+new".into(),
        diff_truncated: false,
    }
}

// ============================== Landing =====================================

#[test]
fn landing_default_renders() {
    draw_landing(&LandingState::default(), &StatusStore::new());
}

#[test]
fn landing_all_loading_renders() {
    let mut s = StatusStore::new();
    s.mark_all_loading();
    draw_landing(&LandingState::default(), &s);
}

#[test]
fn landing_populated_renders() {
    draw_landing(&LandingState::default(), &populated_status());
}

#[test]
fn landing_with_missing_tools_renders() {
    let mut s = StatusStore::new();
    s.set_version(VersionResult {
        kind: VersionKind::Jj,
        status: ToolStatus::Missing,
    });
    s.set_version(VersionResult {
        kind: VersionKind::Tea,
        status: ToolStatus::Errored {
            message: "permission denied".into(),
        },
    });
    s.set_llm(LlmHealth::Unreachable {
        message: "connection refused".into(),
    });
    s.set_workspace(WorkspaceInfo::Outside);
    s.set_tea_auth(TeaAuthStatus::None);
    draw_landing(&LandingState::default(), &s);
}

#[test]
fn landing_with_selected_quit_renders() {
    let state = LandingState { selected: 1 };
    draw_landing(&state, &populated_status());
}

// ============================== Generate ====================================

fn generate_with(phase: GeneratePhase) -> GenerateState {
    GenerateState {
        pane: Pane::Menu,
        revset_selected: 0,
        form: teatui::screens::generate::PrForm {
            head: "abcd".into(),
            branch: "add-foo".into(),
            base: "main".into(),
            title: "Add foo".into(),
            description: "Implements foo.".into(),
            ..Default::default()
        },
        phase,
        last_action: None,
    }
}

#[test]
fn generate_idle_renders() {
    draw_generate(&generate_with(GeneratePhase::Idle), &populated_status());
}

#[test]
fn generate_collecting_renders() {
    draw_generate(
        &generate_with(GeneratePhase::Collecting),
        &populated_status(),
    );
}

#[test]
fn generate_generating_renders() {
    draw_generate(
        &generate_with(GeneratePhase::Generating {
            context: sample_context(),
            prompt: sample_prompt(),
        }),
        &populated_status(),
    );
}

#[test]
fn generate_draft_ready_renders() {
    draw_generate(
        &generate_with(GeneratePhase::DraftReady {
            draft: sample_draft(),
            prompt: sample_prompt(),
        }),
        &populated_status(),
    );
}

#[test]
fn generate_executing_renders() {
    draw_generate(
        &generate_with(GeneratePhase::Executing {
            draft: sample_draft(),
        }),
        &populated_status(),
    );
}

#[test]
fn generate_done_renders() {
    draw_generate(
        &generate_with(GeneratePhase::Done {
            url: "https://gitea.example.com/o/r/pulls/1".into(),
        }),
        &populated_status(),
    );
}

#[test]
fn generate_done_with_action_hint_renders() {
    let mut s = generate_with(GeneratePhase::Done {
        url: "https://gitea.example.com/o/r/pulls/1".into(),
    });
    s.last_action = Some("copied to clipboard");
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_failed_renders() {
    draw_generate(
        &generate_with(GeneratePhase::Failed {
            message: "ollama unreachable".into(),
        }),
        &populated_status(),
    );
}

#[test]
fn generate_with_no_revsets_renders() {
    let mut status = populated_status();
    status.set_revsets(Revsets::Loaded(vec![]));
    draw_generate(&generate_with(GeneratePhase::Idle), &status);
}

#[test]
fn generate_with_revsets_loading_renders() {
    let mut status = populated_status();
    status.revsets = Cached::Loading;
    draw_generate(&generate_with(GeneratePhase::Idle), &status);
}

#[test]
fn generate_with_revsets_errored_renders() {
    let mut status = populated_status();
    status.set_revsets(Revsets::Errored {
        message: "no jj workspace".into(),
    });
    draw_generate(&generate_with(GeneratePhase::Idle), &status);
}

#[test]
fn generate_each_pane_focus_renders() {
    for pane in [Pane::Menu, Pane::Form, Pane::Preview] {
        let mut s = generate_with(GeneratePhase::Idle);
        s.pane = pane;
        draw_generate(&s, &populated_status());
    }
}

// ============================== Build path ==================================

#[test]
fn build_prompt_then_render_does_not_panic() {
    // Stitch the pure-logic prompt builder into a Draft-ready render so any
    // accidental divergence between the two surfaces here.
    let ctx = sample_context();
    let prompt = build_prompt(&ctx);
    let state = generate_with(GeneratePhase::DraftReady {
        draft: sample_draft(),
        prompt,
    });
    draw_generate(&state, &populated_status());
}
