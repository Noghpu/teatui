//! Render-path smoke tests. The contract is: every screen, in every phase
//! we ship, must render against a `TestBackend` without panicking. These
//! tests are deliberately not assertions on exact output — they catch
//! layout / widget construction errors and overflows.

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use teatui::domain::{
    BulkPhase, PrStatus, StackDraft, StackExistingPr, StackIntent, StackPlan, StackPlanItem,
    StackPrInput, annotate_blockers,
};
use teatui::domain::{
    ChangeContext, ContextBundle, DiffContext, GeneratedDraft, JjOp, JjOpKind, LlmHealth,
    PromptBuild, PromptForm, PromptManifest, PromptSection, RevsetSummary, Revsets, StatusStore,
    TeaAuthStatus, ToolStatus, VersionKind, VersionResult, WorkspaceInfo, build_prompt,
};
use teatui::runtime::Cached;
use teatui::screens::generate::{
    BulkItemEditor, BulkItemField, BulkReviewFocus, CommandPreview, FieldId, GeneratePhase,
    GenerateState, InputMode, JjOpDialog, Pane, PendingJjOp,
};
use teatui::screens::{self, LandingState};

fn term() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(120, 30)).expect("terminal")
}

fn small_term() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(80, 24)).expect("terminal")
}

fn draw_landing(state: &LandingState, status: &StatusStore) {
    let mut t = term();
    t.draw(|frame| {
        let area = frame.area();
        screens::landing::render(state, status, frame, area);
    })
    .expect("draw");
}

fn draw_landing_small(state: &LandingState, status: &StatusStore) {
    let mut t = small_term();
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

fn draw_generate_small(state: &GenerateState, status: &StatusStore) {
    let mut t = small_term();
    t.draw(|frame| {
        let area = frame.area();
        screens::generate::render(state, status, frame, area);
    })
    .expect("draw");
}

fn generate_small_text(state: &GenerateState, status: &StatusStore) -> String {
    let mut t = small_term();
    t.draw(|frame| {
        let area = frame.area();
        screens::generate::render(state, status, frame, area);
    })
    .expect("draw");
    buffer_text(t.backend().buffer())
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    buffer
        .content()
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
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
        remote: None,
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
        label: format!("trunk()..{change_id}"),
        change_id: change_id.into(),
        commit_id: "deadbeef".into(),
        bookmarks: vec![],
        description: desc.into(),
        description_body: "Body line".into(),
        author: String::new(),
        stats: "1 file changed, 2 insertions(+), 1 deletion(-)".into(),
        commit_count: 1,
        commit_ids: vec!["deadbeef".into()],
        change_ids: vec![change_id.into()],
        recent_log: vec![format!("deadbeef {desc}")],
        warnings: vec![],
    }
}

fn ordered_revset_status() -> StatusStore {
    let mut status = populated_status();
    status.set_revsets(Revsets::Loaded(vec![
        sample_revset("new", "Newer change"),
        sample_revset("base", "Base change"),
        sample_revset("old", "Older change"),
    ]));
    status
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
        pr_type: "feat".into(),
        branch_slug: "add-foo-to-bar".into(),
        title: "Add foo to bar".into(),
        description: "Implements foo.\n\nDetails follow.".into(),
    }
}

fn sample_commands() -> CommandPreview {
    CommandPreview {
        bookmark: "jj --no-pager bookmark set --allow-backwards add-foo -r abcd".into(),
        push: "jj --no-pager git push --bookmark add-foo".into(),
        create: "tea pr create --base main --head add-foo --title \"Add foo\"".into(),
    }
}

fn sample_context() -> ContextBundle {
    ContextBundle {
        base: "main".into(),
        head: "abcd".into(),
        status: "Working copy : abcd".into(),
        changes: vec![ChangeContext {
            subject: "feat: add foo".into(),
            body: "Adds foo support.".into(),
            diff_stat: "1 file changed".into(),
        }],
        aggregate: DiffContext {
            diff_stat: "1 file changed".into(),
            diff: "@@ -1 +1 @@\n-old\n+new".into(),
            diff_truncated: false,
            diff_omitted: false,
        },
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
    let state = LandingState { selected: 3 };
    draw_landing(&state, &populated_status());
}

#[test]
fn landing_small_terminal_renders() {
    draw_landing_small(&LandingState::default(), &populated_status());
}

// ============================== Generate ====================================

fn generate_with(phase: GeneratePhase) -> GenerateState {
    let mut form = teatui::screens::generate::PrForm::new("main".into());
    form.head.set_value("abcd".into());
    form.branch_name.set_value("add-foo".into());
    form.title.set_value("Add foo".into());
    form.description.set_value("Implements foo.".into());
    GenerateState {
        pane: Pane::Menu,
        revset_selected: 0,
        scroll_menu: std::cell::Cell::new(0),
        scroll_form: std::cell::Cell::new(0),
        scroll_preview: 0,
        input_mode: InputMode::Normal,
        field_focus: FieldId::Head,
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
fn generate_confirming_renders() {
    draw_generate(
        &generate_with(GeneratePhase::Confirming {
            draft: sample_draft(),
            prompt: sample_prompt(),
            commands: sample_commands(),
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
fn generate_jj_mutating_renders() {
    draw_generate(
        &generate_with(GeneratePhase::JjMutating {
            op: JjOpKind::MoveUp,
            summary: "moving abcd Add foo above efgh Base work".into(),
        }),
        &populated_status(),
    );
}

#[test]
fn generate_small_terminal_each_phase_renders() {
    for phase in [
        GeneratePhase::Idle,
        GeneratePhase::Collecting,
        GeneratePhase::Generating {
            context: sample_context(),
            prompt: sample_prompt(),
        },
        GeneratePhase::DraftReady {
            draft: sample_draft(),
            prompt: sample_prompt(),
        },
        GeneratePhase::Confirming {
            draft: sample_draft(),
            prompt: sample_prompt(),
            commands: sample_commands(),
        },
        GeneratePhase::Executing {
            draft: sample_draft(),
        },
        GeneratePhase::JjMutating {
            op: JjOpKind::SquashWithBelow,
            summary: "squashing abcd Add foo into efgh Base work".into(),
        },
        GeneratePhase::Done {
            url: "https://gitea.example.com/o/r/pulls/1".into(),
        },
        GeneratePhase::Failed {
            message: "ollama unreachable".into(),
        },
    ] {
        draw_generate_small(&generate_with(phase), &populated_status());
    }
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
    s.last_action = Some("copied to clipboard".into());
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
fn generate_jj_confirm_dialog_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.jj_op_dialog = Some(JjOpDialog::Confirm(PendingJjOp {
        op: JjOp {
            kind: JjOpKind::MoveDown,
            change_id: "abcd".into(),
            target_id: "efgh".into(),
        },
        change: "abcd Add foo".into(),
        target: "efgh Base work".into(),
    }));
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_jj_error_dialog_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.jj_op_dialog = Some(JjOpDialog::Error {
        title: "move above failed".into(),
        message: "conflicts exist in trunk()..@".into(),
    });
    draw_generate(&s, &populated_status());
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

#[test]
fn generate_each_field_focus_renders() {
    for field_focus in FieldId::ALL {
        let mut s = generate_with(GeneratePhase::Idle);
        s.pane = Pane::Form;
        s.field_focus = field_focus;
        draw_generate(&s, &populated_status());
    }
}

#[test]
fn generate_editing_single_line_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.pane = Pane::Form;
    s.field_focus = FieldId::Title;
    s.input_mode = InputMode::Editing;
    s.form.title.begin_edit();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_editing_multiline_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.pane = Pane::Form;
    s.field_focus = FieldId::Description;
    s.input_mode = InputMode::Editing;
    s.form.description.begin_edit();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_editing_picker_modal_renders() {
    let status = populated_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.ensure_field_options_synced(&status);
    s.pane = Pane::Form;
    s.field_focus = FieldId::Head;
    s.input_mode = InputMode::Editing;
    s.form.head.begin_edit();
    draw_generate(&s, &status);
}

#[test]
fn generate_head_base_order_warning_renders() {
    let status = ordered_revset_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.ensure_field_options_synced(&status);
    s.pane = Pane::Form;
    s.field_focus = FieldId::Head;
    s.form.head.set_value("old".into());
    s.form.base.set_value("base".into());
    draw_generate(&s, &status);
}

#[test]
fn generate_picker_modal_with_discouraged_options_renders() {
    let status = ordered_revset_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.ensure_field_options_synced(&status);
    s.pane = Pane::Form;
    s.field_focus = FieldId::Head;
    s.input_mode = InputMode::Editing;
    s.form.head.set_value("new".into());
    s.form.base.set_value("base".into());
    s.form.head.begin_edit();
    draw_generate(&s, &status);
}

// ====================== Selected heads rendering ============================

#[test]
fn generate_with_zero_selected_heads_renders() {
    // Baseline: no heads selected — same as normal idle state.
    let status = ordered_revset_status();
    let s = generate_with(GeneratePhase::Idle);
    draw_generate(&s, &status);
}

#[test]
fn generate_with_one_selected_head_renders() {
    let status = ordered_revset_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.pane = Pane::Menu;
    s.selected_heads.push("new".into());
    draw_generate(&s, &status);
}

#[test]
fn generate_with_multiple_selected_heads_renders() {
    let status = ordered_revset_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.pane = Pane::Menu;
    s.selected_heads.push("new".into());
    s.selected_heads.push("old".into());
    draw_generate(&s, &status);
}

#[test]
fn generate_selected_head_at_cursor_renders() {
    // Cursor and selected marker both active on the same row.
    let status = ordered_revset_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.pane = Pane::Menu;
    s.revset_selected = 0;
    s.selected_heads.push("new".into());
    draw_generate(&s, &status);
}

#[test]
fn generate_selected_heads_small_terminal_renders() {
    let status = ordered_revset_status();
    let mut s = generate_with(GeneratePhase::Idle);
    s.pane = Pane::Menu;
    s.selected_heads.push("new".into());
    s.selected_heads.push("old".into());
    draw_generate_small(&s, &status);
}

// ========================= Bulk modal phases ================================

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

#[test]
fn generate_bulk_collecting_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.selected_heads.push("new".into());
    s.selected_heads.push("old".into());
    s.bulk = BulkPhase::Collecting;
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_generating_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.selected_heads.push("new".into());
    s.bulk = sample_bulk_generating();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_empty_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Review {
        plan: sample_stack_plan(0),
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_one_item_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Review {
        plan: sample_stack_plan(1),
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_multiple_items_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Review {
        plan: sample_stack_plan(3),
        cursor: 1,
        pushing: None,
        push_all: false,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_preview_focus_with_wrapped_title_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    let mut plan = sample_stack_plan(2);
    plan.items[0].title =
        "Refine the stacked pull request review workflow with a deliberately long generated title"
            .into();
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    s.bulk_review_focus = BulkReviewFocus::Preview;
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_push_in_flight_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    let mut plan = sample_stack_plan(2);
    plan.items[0].status = PrStatus::Created {
        url: "https://example.com/pulls/1".into(),
    };
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 1,
        pushing: Some(1),
        push_all: true,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_done_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    let mut plan = sample_stack_plan(2);
    plan.items[0].status = PrStatus::Created {
        url: "https://example.com/pulls/1".into(),
    };
    plan.items[1].status = PrStatus::Created {
        url: "https://example.com/pulls/2".into(),
    };
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 1,
        pushing: None,
        push_all: false,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_failed_item_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    let mut plan = sample_stack_plan(2);
    plan.items[0].status = PrStatus::Created {
        url: "https://example.com/pulls/1".into(),
    };
    plan.items[1].status = PrStatus::Failed {
        step: teatui::domain::ExecuteStep::Create,
        message: "validation failed".into(),
    };
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 1,
        pushing: None,
        push_all: false,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_long_push_error_stays_readable() {
    let mut s = generate_with(GeneratePhase::Idle);
    let mut plan = sample_stack_plan(2);
    plan.items[1].status = PrStatus::Failed {
        step: teatui::domain::ExecuteStep::Create,
        message: "tea pr create rejected the request because the remote returned a detailed validation message and the final-token-visible marker must remain readable".into(),
    };
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 1,
        pushing: None,
        push_all: false,
    };
    s.bulk_review_focus = BulkReviewFocus::Preview;
    s.seed_bulk_editor_from_cursor();
    s.bulk_editor.field_focus = BulkItemField::Description;

    let text = generate_small_text(&s, &populated_status());
    assert!(text.contains("final-token-visible"), "{text}");
}

#[test]
fn generate_bulk_failed_renders() {
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Failed {
        message: "LLM server unreachable".into(),
    };
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_with_bookmark_collision_blocker_renders() {
    let mut plan = sample_stack_plan(2);
    annotate_blockers(&mut plan, &["pr/feat/pr-0".to_string()], &[]);
    assert!(!plan.items[0].blockers.is_empty());
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_with_existing_pr_blocker_renders() {
    let mut plan = sample_stack_plan(1);
    annotate_blockers(
        &mut plan,
        &[],
        &[StackExistingPr {
            head_branch: "pr/feat/pr-0".into(),
            state: "open".into(),
            url: Some("https://example.com/pulls/17".into()),
        }],
    );
    assert!(!plan.items[0].blockers.is_empty());
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    s.seed_bulk_editor_from_cursor();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_small_terminal_each_phase_renders() {
    let phases: Vec<BulkPhase> = vec![
        BulkPhase::Collecting,
        sample_bulk_generating(),
        BulkPhase::Review {
            plan: sample_stack_plan(2),
            cursor: 0,
            pushing: None,
            push_all: false,
        },
        BulkPhase::Failed {
            message: "timeout".into(),
        },
    ];
    for phase in phases {
        let mut s = generate_with(GeneratePhase::Idle);
        s.selected_heads.push("new".into());
        s.bulk = phase;
        s.seed_bulk_editor_from_cursor();
        draw_generate_small(&s, &populated_status());
    }
}

#[test]
fn generate_bulk_review_description_editing_renders() {
    // Exercise the shared render_text_field path for the Description field
    // in the bulk modal while editing — the TextArea widget (with cursor) must
    // be drawn rather than a static preview.
    let mut s = generate_with(GeneratePhase::Idle);
    s.bulk = BulkPhase::Review {
        plan: sample_stack_plan(2),
        cursor: 1,
        pushing: None,
        push_all: false,
    };
    s.bulk_review_focus = BulkReviewFocus::Preview;
    s.seed_bulk_editor_from_cursor();
    s.bulk_editor.field_focus = BulkItemField::Description;
    s.bulk_editor.editing = true;
    s.bulk_editor.description.begin_edit();
    draw_generate(&s, &populated_status());
}

#[test]
fn generate_bulk_review_long_description_stays_visible_when_scrolled() {
    let mut s = generate_with(GeneratePhase::Idle);
    let mut plan = sample_stack_plan(1);
    plan.items[0].description = (0..36)
        .map(|i| format!("description line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    s.bulk = BulkPhase::Review {
        plan,
        cursor: 0,
        pushing: None,
        push_all: false,
    };
    s.bulk_review_focus = BulkReviewFocus::Preview;
    s.seed_bulk_editor_from_cursor();
    s.bulk_editor.field_focus = BulkItemField::Description;

    let text = generate_small_text(&s, &populated_status());
    assert!(text.contains("description line 35"), "{text}");

    s.bulk_editor.editing = true;
    s.bulk_editor.description.begin_edit();
    let text = generate_small_text(&s, &populated_status());
    assert!(text.contains("description line 35"), "{text}");
}

// ========================== Backend switcher ================================

fn sample_backends() -> Vec<teatui::config::LlmBackend> {
    use teatui::config::LlmBackend;
    vec![
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
    ]
}

#[test]
fn backend_picker_mixed_health_renders() {
    use teatui::screens::backend_picker::{self, BackendPicker};

    let backends = sample_backends();
    let mut status = populated_status();
    // One reachable (✓ normal), one unreachable (✗ warning), one still
    // in-flight (◌ faded) to exercise all three row styles at once.
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
    let mut t = term();
    t.draw(|frame| {
        let area = frame.area();
        backend_picker::render(&picker, &backends, "default", &status, frame, area);
    })
    .expect("draw");
}

#[test]
fn backend_picker_over_small_terminal_renders() {
    use teatui::screens::backend_picker::{self, BackendPicker};

    let backends = sample_backends();
    let status = StatusStore::new(); // nothing probed yet — all pending
    let picker = BackendPicker::new("missing", &backends);
    let mut t = small_term();
    t.draw(|frame| {
        let area = frame.area();
        backend_picker::render(&picker, &backends, "default", &status, frame, area);
    })
    .expect("draw");
}

// ============================== Build path ==================================

#[test]
fn build_prompt_then_render_does_not_panic() {
    // Stitch the pure-logic prompt builder into a Draft-ready render so any
    // accidental divergence between the two surfaces here.
    let ctx = sample_context();
    let prompt = build_prompt(
        &ctx,
        &PromptForm {
            head: "abcd".into(),
            base: "main".into(),
            branch: "add-foo".into(),
            title: "Add foo".into(),
            description: "Body".into(),
        },
    );
    let state = generate_with(GeneratePhase::DraftReady {
        draft: sample_draft(),
        prompt,
    });
    draw_generate(&state, &populated_status());
}
