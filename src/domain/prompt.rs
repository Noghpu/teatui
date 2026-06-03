use super::context::ContextBundle;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBuild {
    pub prompt: String,
    pub manifest: PromptManifest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptForm {
    pub head: String,
    pub base: String,
    pub branch: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptManifest {
    pub sections: Vec<PromptSection>,
    pub total_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSection {
    pub name: &'static str,
    pub bytes: usize,
}

const SYSTEM: &str = "You are a PR drafting assistant. Draft a pull request for the supplied `head` against `base`. All log and diff context is for `base..head`.\nRespond with ONLY the requested JSON object — no markdown fences, no prose before or after.\n\n";

const INSTRUCTIONS: &str = "\
Use `changes` (commit subjects, bodies, and per-change diff stats, ordered oldest-to-newest) to understand the sequence and motivation behind the work.
Use `aggregate.diff` — the single unified (git-format) diff of head against base — as the source of truth for what actually changed. `changes` describe the journey; `aggregate` is the destination.
Explain what changed and why. You may infer motivation from commit messages, file names, and code relationships, but do not invent tests, issue numbers, benchmarks, user reports, or external facts not present in the context.
If `aggregate.diff_truncated` is true the diff was cut to fit a size budget; rely more on the commit bodies and diff stats for the unseen parts, and do not claim full coverage of changes you cannot see.
";

/// Used when the backend omits the diff (`diff_budget_bytes = 0`). No code diff
/// is present, so the model must work from commit messages and the per-file
/// stat summary alone — and must not pretend to line-level certainty.
const INSTRUCTIONS_NO_DIFF: &str = "\
No code diff is included in this context — the backend is configured to omit it (`aggregate.diff_omitted` is true). Work from `changes` (commit subjects, bodies, and per-change diff stats, ordered oldest-to-newest) and `aggregate.diff_stat` (the per-file change summary) as your sources of truth.
Explain what changed and why, leaning on the commit messages for motivation and the diff stats for which files and roughly how much changed. You may infer intent from commit messages and file names, but do not invent tests, issue numbers, benchmarks, user reports, or external facts not present in the context.
Because you cannot see the actual code changes, keep claims at the level the commit messages and stats support; do not assert line-level detail or full coverage you cannot verify.
";

const INPUT_SCHEMA: &str = "\
Incoming context JSON:
- task.base: target branch/revision the pull request is opened against.
- task.head: source revision being proposed.
- form.branch: current branch field value, if any. Treat as user context, not a required output format.
- form.title: current title field value, if any. Refine it when the context supports a better title.
- form.description: current description field value, if any. Refine it when the context supports a better body.
- workspace.status: raw `jj status` output at generation time. Present only when head is the working copy; empty otherwise.
- changes: ordered oldest-to-newest; each item has subject, body, and diff_stat. No per-change diff is included — the code is carried once by aggregate. Change ids and commit ids are intentionally omitted.
- aggregate: the net effect of head against base. diff_stat is the per-file summary. When the diff fits the budget it is in `diff` (the full unified git-format diff), with `diff_truncated` true if it was cut to fit. When the backend omits the diff, `diff` is absent and `diff_omitted` is true — rely on diff_stat and the commit bodies instead.
";

const OUTPUT_SCHEMA: &str = r#"Output JSON schema:
{
  "type": "feat|fix|docs|refactor|test|chore",
  "branch_slug": "lowercase-kebab-case-descriptive-name",
  "title": "imperative title under 72 chars, no trailing period",
  "description": "markdown body with ## Summary, ## Why, and ## Verification sections"
}

Field rules:
- type: choose exactly one of feat, fix, docs, refactor, test, chore.
- branch_slug: lowercase ASCII kebab-case only; no `pr/` prefix and no type prefix. The app will construct `pr/{type}/{branch_slug}`.
- title: one-line imperative summary, <=72 chars, no trailing period.
- description: markdown body. Use this structure:
  ## Summary
  - One to three bullets describing what changed.

  ## Why
  A short paragraph explaining the motivation supported by the context.

  ## Verification
  - Not run (not provided)

Only replace the Verification bullet when the context explicitly provides verification evidence.
"#;

pub fn build_prompt(ctx: &ContextBundle, form: &PromptForm) -> PromptBuild {
    let mut buf = String::new();
    let mut sections = Vec::new();
    buf.push_str(SYSTEM);

    let instructions = if ctx.aggregate.diff_omitted {
        INSTRUCTIONS_NO_DIFF
    } else {
        INSTRUCTIONS
    };
    push_section(&mut buf, &mut sections, "Instructions", instructions);
    push_section(
        &mut buf,
        &mut sections,
        "Incoming Data Schema",
        INPUT_SCHEMA,
    );
    push_section(&mut buf, &mut sections, "Output JSON Schema", OUTPUT_SCHEMA);
    push_section(
        &mut buf,
        &mut sections,
        "Context JSON",
        &render_context_json(ctx, form),
    );

    let total_bytes = buf.len();
    PromptBuild {
        prompt: buf,
        manifest: PromptManifest {
            sections,
            total_bytes,
        },
    }
}

fn push_section(
    buf: &mut String,
    sections: &mut Vec<PromptSection>,
    name: &'static str,
    content: &str,
) {
    buf.push_str("## ");
    buf.push_str(name);
    buf.push('\n');
    let start = buf.len();
    let trimmed = content.trim();
    if trimmed.is_empty() {
        buf.push_str("(empty)\n\n");
    } else {
        buf.push_str(trimmed);
        buf.push_str("\n\n");
    }
    sections.push(PromptSection {
        name,
        bytes: buf.len() - start,
    });
}

fn render_context_json(ctx: &ContextBundle, form: &PromptForm) -> String {
    let context = PromptContext {
        task: PromptTask {
            base: first_non_empty(&form.base, &ctx.base),
            head: first_non_empty(&form.head, &ctx.head),
        },
        form: PromptInputForm {
            branch: form.branch.trim(),
            title: form.title.trim(),
            description: form.description.trim(),
        },
        workspace: PromptWorkspace {
            status: ctx.status.trim(),
        },
        changes: ctx
            .changes
            .iter()
            .map(|change| PromptChange {
                subject: change.subject.trim(),
                body: change.body.trim(),
                diff_stat: change.diff_stat.trim(),
            })
            .collect(),
        aggregate: if ctx.aggregate.diff_omitted {
            PromptDiff {
                diff_stat: ctx.aggregate.diff_stat.trim(),
                diff: None,
                diff_truncated: None,
                diff_omitted: true,
            }
        } else {
            PromptDiff {
                diff_stat: ctx.aggregate.diff_stat.trim(),
                diff: Some(ctx.aggregate.diff.trim()),
                diff_truncated: Some(ctx.aggregate.diff_truncated),
                diff_omitted: false,
            }
        },
    };
    serde_json::to_string_pretty(&context).unwrap_or_else(|_| "{}".into())
}

#[derive(Serialize)]
struct PromptContext<'a> {
    task: PromptTask<'a>,
    form: PromptInputForm<'a>,
    workspace: PromptWorkspace<'a>,
    changes: Vec<PromptChange<'a>>,
    aggregate: PromptDiff<'a>,
}

#[derive(Serialize)]
struct PromptTask<'a> {
    base: &'a str,
    head: &'a str,
}

#[derive(Serialize)]
struct PromptInputForm<'a> {
    branch: &'a str,
    title: &'a str,
    description: &'a str,
}

#[derive(Serialize)]
struct PromptWorkspace<'a> {
    status: &'a str,
}

#[derive(Serialize)]
struct PromptChange<'a> {
    subject: &'a str,
    body: &'a str,
    diff_stat: &'a str,
}

#[derive(Serialize)]
struct PromptDiff<'a> {
    diff_stat: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff_truncated: Option<bool>,
    diff_omitted: bool,
}

fn first_non_empty<'a>(preferred: &'a str, fallback: &'a str) -> &'a str {
    let preferred = preferred.trim();
    if preferred.is_empty() {
        fallback.trim()
    } else {
        preferred
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::{ChangeContext, DiffContext};
    use super::*;

    fn sample() -> ContextBundle {
        ContextBundle {
            base: "main".into(),
            head: "abcd".into(),
            status: "Working copy : abcd Test".into(),
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

    fn sample_form() -> PromptForm {
        PromptForm {
            head: "abcd".into(),
            base: "main".into(),
            branch: "add-foo".into(),
            title: "Add foo".into(),
            description: "Existing draft body".into(),
        }
    }

    #[test]
    fn build_prompt_lists_all_sections() {
        let prompt = build_prompt(&sample(), &sample_form());
        let names: Vec<&str> = prompt.manifest.sections.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "Instructions",
                "Incoming Data Schema",
                "Output JSON Schema",
                "Context JSON"
            ]
        );
    }

    #[test]
    fn build_prompt_total_bytes_matches_prompt_len() {
        let prompt = build_prompt(&sample(), &sample_form());
        assert_eq!(prompt.manifest.total_bytes, prompt.prompt.len());
    }

    #[test]
    fn empty_section_renders_placeholder() {
        let mut ctx = sample();
        ctx.aggregate.diff = String::new();
        let prompt = build_prompt(&ctx, &sample_form());
        assert!(prompt.prompt.contains(r#""diff": """#));
    }

    #[test]
    fn pr_inputs_include_current_form_values() {
        let prompt = build_prompt(&sample(), &sample_form());
        assert!(prompt.prompt.contains("## Context JSON\n"));
        assert!(prompt.prompt.contains(r#""base": "main""#));
        assert!(prompt.prompt.contains(r#""head": "abcd""#));
        assert!(prompt.prompt.contains(r#""branch": "add-foo""#));
        assert!(prompt.prompt.contains(r#""title": "Add foo""#));
        assert!(
            prompt
                .prompt
                .contains(r#""description": "Existing draft body""#)
        );
        assert!(prompt.prompt.contains(r#""subject": "feat: add foo""#));
    }

    #[test]
    fn context_json_carries_one_diff_from_aggregate_only() {
        // Per-change diffs are intentionally dropped: the code is sent once via
        // the aggregate. With a single change, exactly one `diff`/`diff_truncated`
        // pair (the aggregate's) should appear in the rendered context.
        let prompt = build_prompt(&sample(), &sample_form());
        assert_eq!(prompt.prompt.matches(r#""diff":"#).count(), 1);
        assert_eq!(prompt.prompt.matches(r#""diff_truncated":"#).count(), 1);
        // The journey is still present per change.
        assert!(prompt.prompt.contains(r#""diff_stat": "1 file changed""#));
    }

    #[test]
    fn omitted_diff_adapts_prompt_and_drops_diff_field() {
        let mut ctx = sample();
        ctx.aggregate.diff = String::new();
        ctx.aggregate.diff_omitted = true;
        let prompt = build_prompt(&ctx, &sample_form());
        // No diff field is rendered; the omission is flagged instead.
        assert_eq!(prompt.prompt.matches(r#""diff":"#).count(), 0);
        assert_eq!(prompt.prompt.matches(r#""diff_truncated":"#).count(), 0);
        assert!(prompt.prompt.contains(r#""diff_omitted": true"#));
        // The instructions switch to the diff-free guidance, and the stat journey
        // is still carried.
        assert!(prompt.prompt.contains("No code diff is included"));
        assert!(prompt.prompt.contains(r#""diff_stat": "1 file changed""#));
    }

    #[test]
    fn present_diff_marks_omitted_false() {
        let prompt = build_prompt(&sample(), &sample_form());
        assert!(prompt.prompt.contains(r#""diff_omitted": false"#));
        assert!(!prompt.prompt.contains("No code diff is included"));
    }

    #[test]
    fn pr_inputs_do_not_include_execution_metadata() {
        let prompt = build_prompt(&sample(), &sample_form());
        assert!(!prompt.prompt.contains("labels:"));
        assert!(!prompt.prompt.contains("assignees:"));
        assert!(!prompt.prompt.contains("milestone:"));
    }

    #[test]
    fn context_json_omits_range_and_revision_ids() {
        let prompt = build_prompt(&sample(), &sample_form());
        assert!(!prompt.prompt.contains("revset"));
        assert!(!prompt.prompt.contains("range"));
        assert!(!prompt.prompt.contains("change_id"));
        assert!(!prompt.prompt.contains("commit_id"));
    }
}
