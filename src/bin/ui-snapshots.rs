use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::{Frame, Terminal};

use teatui::config::{Config, ForgeSelection};
use teatui::domain::{
    BaseBookmark, BulkPhase, ChangeContext, ContextBundle, DiffContext, ForgeAuthStatus,
    GeneratedDraft, JjOp, JjOpKind, LlmHealth, PrStatus, PromptBuild, PromptManifest,
    PromptSection, RemoteInfo, RepoOptions, RevsetSummary, Revsets, StackDraft, StackIntent,
    StackPlan, StackPlanItem, StackPrInput, StatusStore, ToolStatus, VersionKind, VersionResult,
    WorkspaceInfo,
};
use teatui::screens::generate::{
    CommandPreview, FieldId, GeneratePhase, GenerateState, InputMode, JjOpDialog, Pane,
    PendingJjOp, PrForm,
};
use teatui::screens::{self, LandingState};

const DEFAULT_OUT_DIR: &str = "target/ui-snapshots";
const CELL_WIDTH: f32 = 9.6;
const CELL_HEIGHT: f32 = 18.0;
const FONT_SIZE: f32 = 14.0;

#[derive(Debug, Parser)]
#[command(about = "Render deterministic TUI screenshots for visual review")]
struct Args {
    #[arg(long, default_value = DEFAULT_OUT_DIR)]
    out: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum SnapshotKind {
    LandingPopulated,
    LandingManualForge,
    LandingAuthError,
    LandingSmall,
    GenerateIdle,
    GenerateFormFocused,
    GenerateDraftReady,
    GenerateConfirming,
    GenerateJjMutating,
    GenerateJjConfirm,
    GenerateJjError,
    GeneratePickerModal,
    GenerateSmall,
    BackendPicker,
    GenerateBulkCollecting,
    GenerateBulkGenerating,
    GenerateBulkReview,
    GenerateBulkPushCurrent,
    GenerateBulkPushDone,
    GenerateBulkPushFailed,
    GenerateBulkFailed,
    GenerateBulkSmall,
}

#[derive(Debug, Clone, Copy)]
struct SnapshotSpec {
    name: &'static str,
    width: u16,
    height: u16,
    kind: SnapshotKind,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    fs::create_dir_all(&args.out)?;

    let mut written = Vec::new();
    for spec in snapshot_specs() {
        let buffer = render_snapshot(spec)?;
        let text = buffer_to_text(&buffer);
        let svg = buffer_to_svg(&buffer);

        let text_path = args.out.join(format!("{}.txt", spec.name));
        let svg_path = args.out.join(format!("{}.svg", spec.name));
        fs::write(&text_path, text)?;
        fs::write(&svg_path, svg)?;
        written.push((spec.name, text_path, svg_path));
    }

    fs::write(args.out.join("index.html"), index_html(&written))?;
    println!(
        "wrote {} snapshots to {}",
        written.len(),
        args.out.display()
    );
    println!("open {}", args.out.join("index.html").display());
    Ok(())
}

fn snapshot_specs() -> Vec<SnapshotSpec> {
    vec![
        SnapshotSpec {
            name: "landing-populated",
            width: 120,
            height: 30,
            kind: SnapshotKind::LandingPopulated,
        },
        SnapshotSpec {
            name: "landing-manual-forge",
            width: 120,
            height: 30,
            kind: SnapshotKind::LandingManualForge,
        },
        SnapshotSpec {
            name: "landing-auth-error",
            width: 120,
            height: 30,
            kind: SnapshotKind::LandingAuthError,
        },
        SnapshotSpec {
            name: "landing-small",
            width: 80,
            height: 24,
            kind: SnapshotKind::LandingSmall,
        },
        SnapshotSpec {
            name: "generate-idle",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateIdle,
        },
        SnapshotSpec {
            name: "generate-form-focused",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateFormFocused,
        },
        SnapshotSpec {
            name: "generate-draft-ready",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateDraftReady,
        },
        SnapshotSpec {
            name: "generate-confirming",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateConfirming,
        },
        SnapshotSpec {
            name: "generate-jj-mutating",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateJjMutating,
        },
        SnapshotSpec {
            name: "generate-jj-confirm",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateJjConfirm,
        },
        SnapshotSpec {
            name: "generate-jj-error",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateJjError,
        },
        SnapshotSpec {
            name: "generate-picker-modal",
            width: 120,
            height: 30,
            kind: SnapshotKind::GeneratePickerModal,
        },
        SnapshotSpec {
            name: "generate-small",
            width: 80,
            height: 24,
            kind: SnapshotKind::GenerateSmall,
        },
        SnapshotSpec {
            name: "backend-picker",
            width: 120,
            height: 30,
            kind: SnapshotKind::BackendPicker,
        },
        SnapshotSpec {
            name: "generate-bulk-collecting",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkCollecting,
        },
        SnapshotSpec {
            name: "generate-bulk-generating",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkGenerating,
        },
        SnapshotSpec {
            name: "generate-bulk-review",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkReview,
        },
        SnapshotSpec {
            name: "generate-bulk-push-current",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkPushCurrent,
        },
        SnapshotSpec {
            name: "generate-bulk-push-done",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkPushDone,
        },
        SnapshotSpec {
            name: "generate-bulk-push-failed",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkPushFailed,
        },
        SnapshotSpec {
            name: "generate-bulk-failed",
            width: 120,
            height: 30,
            kind: SnapshotKind::GenerateBulkFailed,
        },
        SnapshotSpec {
            name: "generate-bulk-small",
            width: 80,
            height: 24,
            kind: SnapshotKind::GenerateBulkSmall,
        },
    ]
}

fn render_snapshot(spec: SnapshotSpec) -> color_eyre::Result<Buffer> {
    let backend = TestBackend::new(spec.width, spec.height);
    let mut terminal = Terminal::new(backend)?;
    let status = populated_status();
    terminal.draw(|frame| match spec.kind {
        SnapshotKind::LandingPopulated => {
            screens::landing::render(
                &LandingState::default(),
                &status,
                ForgeSelection::Auto,
                frame,
                frame.area(),
            );
        }
        SnapshotKind::LandingManualForge => {
            screens::landing::render(
                &LandingState::default(),
                &status,
                ForgeSelection::Gitea,
                frame,
                frame.area(),
            );
        }
        SnapshotKind::LandingAuthError => {
            let mut status = status.clone();
            status.set_forge_auth(ForgeAuthStatus::Errored {
                message: "token expired".into(),
            });
            screens::landing::render(
                &LandingState::default(),
                &status,
                ForgeSelection::Auto,
                frame,
                frame.area(),
            );
        }
        SnapshotKind::LandingSmall => {
            screens::landing::render(
                &LandingState { selected: 3 },
                &status,
                ForgeSelection::Auto,
                frame,
                frame.area(),
            );
        }
        SnapshotKind::GenerateIdle => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateFormFocused => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Form, FieldId::Description);
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateDraftReady => {
            let mut state = generate_with(
                GeneratePhase::DraftReady {
                    draft: sample_draft(),
                    prompt: sample_prompt(),
                },
                Pane::Preview,
                FieldId::Description,
            );
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateConfirming => {
            let mut state = generate_with(
                GeneratePhase::Confirming {
                    draft: sample_draft(),
                    prompt: sample_prompt(),
                    commands: sample_commands(),
                },
                Pane::Preview,
                FieldId::Description,
            );
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateJjMutating => {
            let mut state = generate_with(
                GeneratePhase::JjMutating {
                    op: JjOpKind::MoveDown,
                    summary: "moving zzzzzzzz restore-pr-ui below yyyyyyyy Add deterministic UI snapshots"
                        .into(),
                },
                Pane::Preview,
                FieldId::Head,
            );
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateJjConfirm => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.ensure_field_options_synced(&status);
            state.jj_op_dialog = Some(JjOpDialog::Confirm(PendingJjOp {
                op: JjOp {
                    kind: JjOpKind::SquashWithBelow,
                    change_id: "zzzzzzzz".into(),
                    target_id: "yyyyyyyy".into(),
                },
                change: "zzzzzzzz restore-pr-ui".into(),
                target: "yyyyyyyy Add deterministic UI snapshots".into(),
            }));
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateJjError => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.ensure_field_options_synced(&status);
            state.jj_op_dialog = Some(JjOpDialog::Error {
                title: "move below failed".into(),
                message: "conflicts exist in trunk()..@".into(),
            });
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GeneratePickerModal => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Form, FieldId::Head);
            state.ensure_field_options_synced(&status);
            state.form.base.set_value("zzzzzzzz".into());
            state.form.head.set_value("yyyyyyyy".into());
            state.input_mode = InputMode::Editing;
            state.form.head.begin_edit();
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateSmall => {
            let mut state = generate_with(
                GeneratePhase::DraftReady {
                    draft: sample_draft(),
                    prompt: sample_prompt(),
                },
                Pane::Preview,
                FieldId::Description,
            );
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::BackendPicker => {
            use teatui::config::LlmBackend;
            use teatui::screens::backend_picker::{self, BackendPicker};

            let backends = vec![
                LlmBackend {
                    name: "default".into(),
                    base_url: "http://localhost:11434".into(),
                    model: "qwen2.5-coder:latest".into(),
                    ..Default::default()
                },
                LlmBackend {
                    name: "fast".into(),
                    base_url: "http://localhost:11500".into(),
                    model: "codellama:7b".into(),
                    ..Default::default()
                },
                LlmBackend {
                    name: "cloud".into(),
                    base_url: "https://api.example.com".into(),
                    model: "gpt-4o-mini".into(),
                    ..Default::default()
                },
            ];
            // Behind the modal: the Generate screen, to show the backdrop.
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());

            // Reachable / unreachable / pending, one of each.
            let mut status = status.clone();
            status.set_backend_health(
                "default".into(),
                LlmHealth::Available {
                    models: vec!["qwen2.5-coder".into()],
                },
            );
            status.set_backend_health(
                "fast".into(),
                LlmHealth::Unreachable {
                    message: "connection refused".into(),
                },
            );
            status.mark_backend_loading("cloud");

            let picker = BackendPicker::new("default", &backends);
            backend_picker::render(&picker, &backends, "default", &status, frame, frame.area());
        }
        SnapshotKind::GenerateBulkCollecting => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.selected_heads.push("zzzzzzzz".into());
            state.selected_heads.push("yyyyyyyy".into());
            state.bulk = BulkPhase::Collecting;
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateBulkGenerating => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.selected_heads.push("zzzzzzzz".into());
            state.selected_heads.push("yyyyyyyy".into());
            state.bulk = sample_bulk_generating();
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateBulkReview => {
            render_bulk_review_snapshot(frame, &status);
        }
        SnapshotKind::GenerateBulkPushCurrent => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.selected_heads.push("zzzzzzzz".into());
            state.selected_heads.push("yyyyyyyy".into());
            let mut plan = sample_stack_plan(2);
            plan.items[0].status = PrStatus::Created {
                url: "https://example.com/pulls/1".into(),
            };
            state.bulk = BulkPhase::Review {
                plan,
                cursor: 1,
                pushing: Some(1),
                push_all: false,
            };
            state.seed_bulk_editor_from_cursor();
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateBulkPushDone => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.selected_heads.push("zzzzzzzz".into());
            state.selected_heads.push("yyyyyyyy".into());
            let mut plan = sample_stack_plan(2);
            plan.items[0].status = PrStatus::Created {
                url: "https://example.com/pulls/1".into(),
            };
            plan.items[1].status = PrStatus::Created {
                url: "https://example.com/pulls/2".into(),
            };
            state.bulk = BulkPhase::Review {
                plan,
                cursor: 1,
                pushing: None,
                push_all: false,
            };
            state.seed_bulk_editor_from_cursor();
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateBulkPushFailed => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.selected_heads.push("zzzzzzzz".into());
            state.selected_heads.push("yyyyyyyy".into());
            let mut plan = sample_stack_plan(2);
            plan.items[0].status = PrStatus::Created {
                url: "https://example.com/pulls/1".into(),
            };
            plan.items[1].status = PrStatus::Failed {
                step: teatui::domain::ExecuteStep::Create,
                message: "http 400: validation failed".into(),
            };
            state.bulk = BulkPhase::Review {
                plan,
                cursor: 1,
                pushing: None,
                push_all: false,
            };
            state.seed_bulk_editor_from_cursor();
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateBulkFailed => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.bulk = BulkPhase::Failed {
                message: "LLM server unreachable: connection refused".into(),
            };
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
        SnapshotKind::GenerateBulkSmall => {
            let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
            state.selected_heads.push("zzzzzzzz".into());
            state.bulk = BulkPhase::Review {
                plan: sample_stack_plan(2),
                cursor: 0,
                pushing: None,
                push_all: false,
            };
            state.seed_bulk_editor_from_cursor();
            state.ensure_field_options_synced(&status);
            screens::generate::render(&state, &status, &Config::default(), frame, frame.area());
        }
    })?;
    Ok(terminal.backend().buffer().clone())
}

fn render_bulk_review_snapshot(frame: &mut Frame, status: &StatusStore) {
    let mut state = generate_with(GeneratePhase::Idle, Pane::Menu, FieldId::Head);
    state.selected_heads.push("zzzzzzzz".into());
    state.selected_heads.push("yyyyyyyy".into());
    let mut plan = sample_stack_plan(2);
    plan.items[0].title =
        "Refine stacked review modal focus with a title that wraps naturally".into();
    state.bulk = BulkPhase::Review {
        plan,
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    state.seed_bulk_editor_from_cursor();
    state.ensure_field_options_synced(status);
    screens::generate::render(&state, status, &Config::default(), frame, frame.area());
}

fn populated_status() -> StatusStore {
    let mut status = StatusStore::new();
    status.set_version(VersionResult {
        kind: VersionKind::Jj,
        status: ToolStatus::Available {
            version: "jj 0.41.0".into(),
        },
    });
    status.set_version(VersionResult {
        kind: VersionKind::Git,
        status: ToolStatus::Available {
            version: "git 2.49.0".into(),
        },
    });
    status.set_version(VersionResult {
        kind: VersionKind::Forge,
        status: ToolStatus::Available {
            version: "tea 0.14.0".into(),
        },
    });
    status.set_forge_label("tea");
    status.set_workspace(WorkspaceInfo::Inside {
        root: PathBuf::from("/home/dev/projects/teatui"),
        remote: Some(RemoteInfo {
            host: "gitea.example.com".into(),
            owner: "owner".into(),
            repo: "teatui".into(),
        }),
        remote_name: Some("origin".into()),
    });
    status.set_forge_auth(ForgeAuthStatus::Configured {
        logins: vec!["gitea".into()],
    });
    status.set_llm(LlmHealth::Available {
        models: vec!["qwen2.5-coder:7b".into()],
    });
    status.set_revsets(Revsets::Loaded(vec![
        sample_revset(
            "zzzzzzzz",
            "9f4c2a1b",
            vec!["restore-pr-ui".into()],
            "Restore previous PR generation UI",
            "Recreate the pre-rewrite pane structure.\nKeep the new runtime cache model intact.",
            "4 files changed, 188 insertions(+), 34 deletions(-)",
        ),
        sample_revset(
            "yyyyyyyy",
            "8e3d1a0c",
            Vec::new(),
            "Add deterministic UI snapshots",
            "Render known screen states to SVG and text artifacts.",
            "2 files changed, 260 insertions(+)",
        ),
    ]));
    status.set_base_bookmarks(vec![
        BaseBookmark {
            name: "main".into(),
            remote: Some("origin".into()),
            is_remote: true,
        },
        BaseBookmark {
            name: "develop".into(),
            remote: Some("origin".into()),
            is_remote: true,
        },
    ]);
    status.set_repo_options(RepoOptions {
        labels: vec!["ui".into(), "rewrite".into(), "bug".into()],
        assignees: vec!["alice".into(), "reviewer".into()],
        milestones: vec!["rewrite".into()],
    });
    status
}

fn sample_revset(
    change_id: &str,
    commit_id: &str,
    bookmarks: Vec<String>,
    description: &str,
    description_body: &str,
    stats: &str,
) -> RevsetSummary {
    RevsetSummary {
        label: format!("trunk()..{change_id}"),
        change_id: change_id.into(),
        commit_id: commit_id.into(),
        bookmarks,
        description: description.into(),
        description_body: description_body.into(),
        author: String::new(),
        stats: stats.into(),
        commit_count: 1,
        commit_ids: vec![commit_id.into()],
        change_ids: vec![change_id.into()],
        recent_log: vec![format!("{commit_id} {description}")],
        warnings: Vec::new(),
    }
}

fn generate_with(phase: GeneratePhase, pane: Pane, field_focus: FieldId) -> GenerateState {
    let mut form = PrForm::new("main@origin".into());
    form.head.set_value("zzzzzzzz".into());
    form.branch_name.set_value("restore-pr-ui".into());
    form.title
        .set_value("Restore previous PR generation UI".into());
    form.description.set_value(
        "Recreate the pre-rewrite pane structure.\n\nKeep the new runtime cache model intact."
            .into(),
    );
    form.labels.set_values(vec!["ui".into(), "rewrite".into()]);
    form.assignees.set_values(vec!["alice".into()]);
    use teatui::screens::generate::{BulkItemEditor, BulkReviewFocus};
    GenerateState {
        pane,
        revset_selected: 0,
        scroll_menu: std::cell::Cell::new(0),
        scroll_form: std::cell::Cell::new(0),
        scroll_preview: 0,
        input_mode: InputMode::Normal,
        field_focus,
        form,
        phase,
        jj_op_dialog: None,
        last_action: None,
        selected_heads: Vec::new(),
        bulk: BulkPhase::Idle,
        bulk_review_focus: BulkReviewFocus::List,
        bulk_editor: BulkItemEditor::default(),
        bulk_list_scroll: std::cell::Cell::new(0),
        bulk_form_scroll: std::cell::Cell::new(0),
        bulk_messages_scroll: std::cell::Cell::new(0),
    }
}

fn sample_stack_plan(n: usize) -> StackPlan {
    let items = (0..n)
        .map(|i| {
            let base = if i == 0 {
                "main".to_string()
            } else {
                format!("pr/feat/pr-{}", i - 1)
            };
            StackPlanItem {
                input: StackPrInput {
                    index: i,
                    base,
                    head: format!("head-{i}"),
                    included_change_ids: vec![format!("ch-{i}")],
                    subject: format!("Change {i}"),
                },
                bookmark: format!("pr/feat/pr-{i}"),
                title: format!("PR {i}: Feature slice"),
                description: format!("Description for PR {i}.\n\n## Summary\n- Does something."),
                status: PrStatus::Pending,
                warnings: Vec::new(),
                blockers: Vec::new(),
                reuse_notes: Vec::new(),
            }
        })
        .collect();
    StackPlan {
        items,
        labels: vec!["ui".into()],
        assignees: Vec::new(),
        milestone: "v1.0".into(),
        intent: StackIntent {
            title: "Add feature".into(),
            description: "Overall goal.".into(),
            branch: "add-feature".into(),
        },
    }
}

fn sample_bulk_generating() -> BulkPhase {
    let inputs: Vec<StackPrInput> = (0..3)
        .map(|i| StackPrInput {
            index: i,
            base: if i == 0 {
                "main".into()
            } else {
                format!("pr/feat/pr-{}", i - 1)
            },
            head: format!("head-{i}"),
            included_change_ids: vec![format!("ch-{i}")],
            subject: format!("Change {i}"),
        })
        .collect();
    let mut drafts = vec![None, None, None];
    drafts[0] = Some(StackDraft {
        index: 0,
        pr_type: "feat".into(),
        branch_slug: "pr-0".into(),
        title: "PR 0".into(),
        description: "Description".into(),
    });
    BulkPhase::Generating {
        prefix: std::sync::Arc::from("PREFIX"),
        inputs,
        intent: StackIntent {
            title: "Add feature".into(),
            description: "Overall goal.".into(),
            branch: "add-feature".into(),
        },
        labels: vec!["ui".into()],
        assignees: Vec::new(),
        milestone: "v1.0".into(),
        drafts,
        warnings: vec![Vec::new(), vec!["LLM fallback: timeout".into()], Vec::new()],
        next: 2,
        total: 3,
    }
}

fn sample_prompt() -> PromptBuild {
    PromptBuild {
        prompt: "PROMPT BODY".into(),
        manifest: PromptManifest {
            sections: vec![
                PromptSection {
                    name: "Status",
                    bytes: 88,
                },
                PromptSection {
                    name: "Log",
                    bytes: 420,
                },
                PromptSection {
                    name: "Diff stat",
                    bytes: 96,
                },
                PromptSection {
                    name: "Diff",
                    bytes: 8192,
                },
            ],
            total_bytes: 8796,
        },
    }
}

fn sample_draft() -> GeneratedDraft {
    GeneratedDraft {
        pr_type: "feat".into(),
        branch_slug: "restore-pr-ui".into(),
        title: "Restore previous PR generation UI".into(),
        description: "Recreates the pre-rewrite PR generation pane layout and information order.\n\nThe implementation keeps cached domain data as the source for rendering."
            .into(),
    }
}

fn sample_commands() -> CommandPreview {
    CommandPreview {
        bookmark: "jj --no-pager bookmark set --allow-backwards restore-pr-ui -r zzzzzzzz".into(),
        push: "jj --no-pager git push --bookmark restore-pr-ui".into(),
        create: "tea pr create --base main --head restore-pr-ui --title \"Restore previous PR generation UI\" --description <description>"
            .into(),
    }
}

#[allow(dead_code)]
fn sample_context() -> ContextBundle {
    ContextBundle {
        base: "main".into(),
        head: "zzzzzzzz".into(),
        status: "Working copy : zzzzzzzz".into(),
        changes: vec![ChangeContext {
            subject: "feat: restore previous PR generation UI".into(),
            body: "Restores the prior Generate screen layout.".into(),
            diff_stat: "4 files changed, 188 insertions(+), 34 deletions(-)".into(),
        }],
        aggregate: DiffContext {
            diff_stat: "4 files changed, 188 insertions(+), 34 deletions(-)".into(),
            diff: "@@ -1 +1 @@\n-old\n+new".into(),
            diff_truncated: false,
            diff_omitted: false,
        },
    }
}

fn buffer_to_text(buffer: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn buffer_to_svg(buffer: &Buffer) -> String {
    let width = f32::from(buffer.area.width) * CELL_WIDTH;
    let height = f32::from(buffer.area.height) * CELL_HEIGHT;
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}">"#
    ));
    out.push('\n');
    out.push_str(&format!(
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        color_hex(Color::Rgb(30, 30, 46), Color::Reset)
    ));
    out.push('\n');
    out.push_str(&format!(
        r#"<g font-family="Cascadia Mono, JetBrains Mono, Menlo, Consolas, monospace" font-size="{FONT_SIZE}" dominant-baseline="text-before-edge">"#
    ));
    out.push('\n');

    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let Some(cell) = buffer.cell((x, y)) else {
                continue;
            };
            let mut fg = cell.fg;
            let mut bg = cell.bg;
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }

            if bg != Color::Reset {
                out.push_str(&format!(
                    r#"<rect x="{:.1}" y="{:.1}" width="{CELL_WIDTH:.1}" height="{CELL_HEIGHT:.1}" fill="{}"/>"#,
                    f32::from(x) * CELL_WIDTH,
                    f32::from(y) * CELL_HEIGHT,
                    color_hex(bg, Color::Rgb(30, 30, 46)),
                ));
                out.push('\n');
            }

            let symbol = cell.symbol();
            if symbol == " " || cell.modifier.contains(Modifier::HIDDEN) {
                continue;
            }

            let weight = if cell.modifier.contains(Modifier::BOLD) {
                "700"
            } else {
                "400"
            };
            let style = text_decoration(cell.modifier);
            let opacity = if cell.modifier.contains(Modifier::DIM) {
                "0.68"
            } else {
                "1"
            };
            out.push_str(&format!(
                r#"<text x="{:.1}" y="{:.1}" fill="{}" font-weight="{weight}" opacity="{opacity}"{}>{}</text>"#,
                f32::from(x) * CELL_WIDTH,
                f32::from(y) * CELL_HEIGHT + 2.0,
                color_hex(fg, Color::Rgb(205, 214, 244)),
                style,
                escape_xml(symbol),
            ));
            out.push('\n');
        }
    }

    out.push_str("</g>\n</svg>\n");
    out
}

fn color_hex(color: Color, fallback: Color) -> String {
    match color {
        Color::Reset => color_hex(fallback, Color::Rgb(205, 214, 244)),
        Color::Black => "#000000".into(),
        Color::Red => "#cd3131".into(),
        Color::Green => "#0dbc79".into(),
        Color::Yellow => "#e5e510".into(),
        Color::Blue => "#2472c8".into(),
        Color::Magenta => "#bc3fbc".into(),
        Color::Cyan => "#11a8cd".into(),
        Color::Gray => "#e5e5e5".into(),
        Color::DarkGray => "#666666".into(),
        Color::LightRed => "#f14c4c".into(),
        Color::LightGreen => "#23d18b".into(),
        Color::LightYellow => "#f5f543".into(),
        Color::LightBlue => "#3b8eea".into(),
        Color::LightMagenta => "#d670d6".into(),
        Color::LightCyan => "#29b8db".into(),
        Color::White => "#ffffff".into(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(_) => color_hex(fallback, Color::Rgb(205, 214, 244)),
    }
}

fn text_decoration(modifier: Modifier) -> &'static str {
    if modifier.contains(Modifier::UNDERLINED) {
        r#" text-decoration="underline""#
    } else if modifier.contains(Modifier::CROSSED_OUT) {
        r#" text-decoration="line-through""#
    } else {
        ""
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn index_html(written: &[(&str, PathBuf, PathBuf)]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<meta charset=\"utf-8\">\n");
    out.push_str("<title>teatui UI snapshots</title>\n");
    out.push_str("<style>body{margin:24px;background:#181825;color:#cdd6f4;font:14px system-ui,sans-serif}article{margin:0 0 32px}img{max-width:100%;border:1px solid #313244}a{color:#89b4fa}</style>\n");
    out.push_str("<h1>teatui UI snapshots</h1>\n");
    for (name, text_path, svg_path) in written {
        let text_name = file_name(text_path);
        let svg_name = file_name(svg_path);
        out.push_str("<article>\n");
        out.push_str(&format!(
            "<h2>{}</h2>\n<p><a href=\"{}\">svg</a> <a href=\"{}\">txt</a></p>\n<img src=\"{}\" alt=\"{}\">\n",
            escape_xml(name),
            escape_xml(&svg_name),
            escape_xml(&text_name),
            escape_xml(&svg_name),
            escape_xml(name),
        ));
        out.push_str("</article>\n");
    }
    out
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}
