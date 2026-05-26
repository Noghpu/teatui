use crate::context::ContextBundle;
use crate::generate::PrForm;

pub const DEFAULT_PROMPT_BYTE_BUDGET: usize = 12_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBuild {
    pub prompt: String,
    pub manifest: PromptManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptManifest {
    pub selected_revset: String,
    pub base_branch: String,
    pub form_values: PrFormManifest,
    pub included_sections: Vec<PromptSection>,
    pub omitted_sections: Vec<OmittedSection>,
    pub byte_count: usize,
    pub truncation_warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSection {
    pub title: String,
    pub body: String,
    pub byte_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmittedSection {
    pub title: String,
    pub reason: String,
    pub byte_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrFormManifest {
    pub head: String,
    pub branch_name: String,
    pub base: String,
    pub title: String,
    pub description: String,
    pub labels: String,
    pub assignees: String,
    pub milestone: String,
}

impl PromptBuild {
    pub fn new(
        context: &ContextBundle,
        form: &PrForm,
        user_instructions: Option<&str>,
        byte_budget: usize,
    ) -> Self {
        let form_values = PrFormManifest::from_form(form);
        let selected_revset = context.selected_revset.label().to_string();
        let base_branch = form.base.display_value().trim().to_string();

        let sections = build_sections(context, form, user_instructions);
        let prompt_contract = prompt_contract();

        let mut prompt = String::new();
        let mut included_sections = Vec::new();
        let mut omitted_sections = Vec::new();
        let mut truncation_warnings = Vec::new();

        append_section(
            &mut prompt,
            &mut included_sections,
            &mut omitted_sections,
            &mut truncation_warnings,
            "Instructions",
            &prompt_contract,
            byte_budget,
        );

        for section in sections {
            append_section(
                &mut prompt,
                &mut included_sections,
                &mut omitted_sections,
                &mut truncation_warnings,
                &section.title,
                &section.body,
                byte_budget,
            );
        }

        let manifest = PromptManifest {
            selected_revset,
            base_branch,
            form_values,
            included_sections,
            omitted_sections,
            byte_count: prompt.len(),
            truncation_warnings,
        };

        Self { prompt, manifest }
    }
}

impl PrFormManifest {
    pub fn from_form(form: &PrForm) -> Self {
        Self {
            head: form.head.display_value().trim().to_string(),
            branch_name: form.branch_name.display_value().trim().to_string(),
            base: form.base.display_value().trim().to_string(),
            title: form.title.display_value().trim().to_string(),
            description: form.description.display_value().trim().to_string(),
            labels: form.labels.display_value().trim().to_string(),
            assignees: form.assignees.display_value().trim().to_string(),
            milestone: form.milestone.display_value().trim().to_string(),
        }
    }
}

struct PromptSectionDraft {
    title: String,
    body: String,
}

fn build_sections(
    context: &ContextBundle,
    form: &PrForm,
    user_instructions: Option<&str>,
) -> Vec<PromptSectionDraft> {
    vec![
        PromptSectionDraft {
            title: "Repository".into(),
            body: repository_section(context),
        },
        PromptSectionDraft {
            title: "Selected jj changes".into(),
            body: selected_changes_section(context),
        },
        PromptSectionDraft {
            title: "Status".into(),
            body: capture_section(&context.status.stdout, &context.status.stderr),
        },
        PromptSectionDraft {
            title: "Log".into(),
            body: capture_section(&context.revset_log.stdout, &context.revset_log.stderr),
        },
        PromptSectionDraft {
            title: "Descriptions".into(),
            body: descriptions_section(context),
        },
        PromptSectionDraft {
            title: "Diff stats".into(),
            body: capture_section(&context.diff_stats.stdout, &context.diff_stats.stderr),
        },
        PromptSectionDraft {
            title: "Diff".into(),
            body: capture_section(&context.diff.stdout, &context.diff.stderr),
        },
        PromptSectionDraft {
            title: "User instructions".into(),
            body: user_instructions
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "No additional instructions were provided.".into()),
        },
        PromptSectionDraft {
            title: "PR form values".into(),
            body: form_values_section(form),
        },
    ]
}

fn prompt_contract() -> String {
    [
        "You are helping write a Gitea pull request for a jj-managed repository.",
        "",
        "Return strict JSON matching this schema:",
        r#"{"branch_name":"feature/example-branch","title":"Short PR title","body":"Markdown PR body","review_notes":["Important inference or uncertainty"]}"#,
        "",
        "Rules:",
        "- Return only the JSON object. Do not wrap it in a Markdown fence or add commentary.",
        "- Use only the context below.",
        "- Do not invent tests, issue links, reviewers, or behavior.",
        "- If context is missing, mention the uncertainty in review_notes.",
        "- Prefer a short branch name with lowercase words separated by hyphens.",
        "- Write a concise title.",
        "- Write a PR body with Summary, Testing, Risks, and Notes sections.",
    ]
    .join("\n")
}

fn repository_section(context: &ContextBundle) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "workspace root: {}",
        context
            .repo_identity
            .workspace_root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(unknown)".into())
    ));
    lines.push(format!(
        "base branch: {}",
        context.repo_identity.base_branch
    ));
    lines.push(format!(
        "selected revset: {}",
        context.repo_identity.selected_revset
    ));

    if let Some(remote) = context.remote.as_ref() {
        lines.push(format!(
            "remote: {} on {}",
            remote.display_name(),
            remote.host
        ));
        if let Some(warning) = remote.warning.as_ref() {
            lines.push(format!("remote warning: {warning}"));
        }
    } else if let Some(remote_url) = context.repo_identity.remote_url.as_ref() {
        lines.push(format!("remote: {}", sanitize_remote_url(remote_url)));
    } else {
        lines.push("remote: unavailable".into());
    }

    lines.join("\n")
}

fn selected_changes_section(context: &ContextBundle) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "revset description: {}",
        context.selected_revset.description()
    ));
    lines.push(format!(
        "commit count: {}",
        context.selected_revset.commit_count()
    ));
    lines.push(format!(
        "bookmarks: {}",
        join_or_placeholder(context.selected_revset.bookmarks(), "(none)")
    ));
    lines.push(format!(
        "commit ids: {}",
        join_or_placeholder(context.selected_revset.commit_ids(), "(none)")
    ));
    lines.push(format!(
        "change ids: {}",
        join_or_placeholder(context.selected_revset.change_ids(), "(none)")
    ));

    if !context.selected_revset.warnings().is_empty() {
        lines.push("warnings:".into());
        for warning in context.selected_revset.warnings() {
            lines.push(format!("- {warning}"));
        }
    }

    if !context.selected_revset.recent_log().is_empty() {
        lines.push("recent log:".into());
        for entry in context.selected_revset.recent_log() {
            lines.push(format!("- {entry}"));
        }
    }

    lines.join("\n")
}

fn descriptions_section(context: &ContextBundle) -> String {
    if context.selected_descriptions.is_empty() {
        return "No selected change descriptions were captured.".into();
    }

    context
        .selected_descriptions
        .iter()
        .enumerate()
        .map(|(index, description)| format!("{}: {}", index + 1, description))
        .collect::<Vec<_>>()
        .join("\n")
}

fn form_values_section(form: &PrForm) -> String {
    let values = PrFormManifest::from_form(form);
    [
        format!("head: {}", values.head),
        format!("branch name: {}", values.branch_name),
        format!("base: {}", values.base),
        format!("title: {}", values.title),
        format!("description: {}", values.description),
        format!("labels: {}", values.labels),
        format!("assignees: {}", values.assignees),
        format!("milestone: {}", values.milestone),
        "treat the entered values above as explicit user intent, not inferred defaults.".into(),
    ]
    .join("\n")
}

fn capture_section(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();

    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "No output.".into(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}\n\nstderr:\n{stderr}"),
    }
}

fn append_section(
    prompt: &mut String,
    included_sections: &mut Vec<PromptSection>,
    omitted_sections: &mut Vec<OmittedSection>,
    truncation_warnings: &mut Vec<String>,
    title: &str,
    body: &str,
    byte_budget: usize,
) {
    if body.trim().is_empty() {
        omitted_sections.push(OmittedSection {
            title: title.into(),
            reason: "section was empty".into(),
            byte_count: 0,
        });
        return;
    }

    let section_text = format!("{title}:\n{body}");
    let remaining = byte_budget.saturating_sub(prompt.len());
    if remaining == 0 {
        omitted_sections.push(OmittedSection {
            title: title.into(),
            reason: "prompt byte budget exhausted".into(),
            byte_count: section_text.len(),
        });
        truncation_warnings.push(format!(
            "omitted {title} because the byte budget was exhausted"
        ));
        return;
    }

    let (section_text, truncated) = truncate_to_budget(&section_text, remaining);
    if section_text.is_empty() {
        omitted_sections.push(OmittedSection {
            title: title.into(),
            reason: "prompt byte budget was too small for the section marker".into(),
            byte_count: body.len(),
        });
        truncation_warnings.push(format!(
            "omitted {title} because the byte budget was too small"
        ));
        return;
    }
    if truncated {
        truncation_warnings.push(format!("truncated {title} to stay within the byte budget"));
    }

    let byte_count = section_text.len();
    prompt.push_str(&section_text);
    prompt.push_str("\n\n");
    included_sections.push(PromptSection {
        title: title.into(),
        body: body.to_string(),
        byte_count,
        truncated,
    });
}

fn truncate_to_budget(text: &str, byte_budget: usize) -> (String, bool) {
    if text.len() <= byte_budget {
        return (text.to_string(), false);
    }

    let marker = "\n\n[truncated]";
    if byte_budget <= marker.len() {
        return (String::new(), true);
    }

    let keep = byte_budget - marker.len();
    let cut = char_boundary(text, keep);
    let mut truncated = String::with_capacity(cut + marker.len());
    truncated.push_str(&text[..cut]);
    truncated.push_str(marker);
    (truncated, true)
}

fn char_boundary(text: &str, mut boundary: usize) -> usize {
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn join_or_placeholder(values: &[String], placeholder: &str) -> String {
    if values.is_empty() {
        placeholder.into()
    } else {
        values.join(", ")
    }
}

fn sanitize_remote_url(raw_url: &str) -> String {
    if let Some((scheme, rest)) = raw_url.split_once("://")
        && let Some((authority, path)) = rest.split_once('/')
    {
        let host = authority
            .rsplit_once('@')
            .map(|(_, host)| host)
            .unwrap_or(authority);
        return format!("{scheme}://{host}/{}", path);
    }

    if let Some((prefix, path)) = raw_url.split_once(':')
        && prefix.contains('@')
    {
        let host = prefix
            .rsplit_once('@')
            .map(|(_, host)| host)
            .unwrap_or(prefix);
        return format!("{host}:{path}");
    }

    raw_url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::generate::PrForm;
    use crate::repo::{
        BaseBranchInfo, BaseBranchSource, LlmBackendStatus, LlmStatus, RemoteInfo, RepoState,
        TeaAuth, ToolStatus,
    };
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn sample_context() -> ContextBundle {
        let config = Config::default();
        let repo = RepoState {
            workspace_root: Some(PathBuf::from("C:/repo")),
            inside_workspace: true,
            jj: ToolStatus::Available,
            git: ToolStatus::Available,
            tea: ToolStatus::Available,
            tea_auth: TeaAuth::Configured {
                host: "code.example.com".into(),
                user: Some("alice".into()),
            },
            remote: Some(RemoteInfo::parse("git@code.example.com:team/project.git")),
            base_branch: BaseBranchInfo {
                name: config.pr.default_base.clone(),
                source: BaseBranchSource::Config,
            },
            llm_active: config.llm.active.clone(),
            llm_backends: vec![LlmBackendStatus {
                name: config.llm.backends[0].name.clone(),
                backend_type: config.llm.backends[0].backend_type.clone(),
                base_url: config.llm.backends[0].base_url.clone(),
                model: config.llm.backends[0].model.clone(),
                status: LlmStatus::Reachable,
            }],
            blockers: Vec::new(),
        };

        ContextBundle {
            repo_identity: crate::context::RepoIdentity {
                collected_at: SystemTime::now(),
                workspace_root: repo.workspace_root.clone(),
                remote_url: repo.remote.as_ref().map(|remote| remote.raw_url.clone()),
                base_branch: repo.base_branch.name.clone(),
                selected_revset: "@".into(),
            },
            remote: repo.remote.clone(),
            form: PrForm::default(),
            selected_revset: crate::generate::RevsetSummary::new(
                "@",
                "Keep the current change",
                vec!["feature/example".into()],
                "1 file changed, 2 insertions(+), 1 deletion(-)",
                1,
                vec!["abc123".into()],
                vec!["def456".into()],
                vec!["abc123 def456 Keep the current change [feature/example]".into()],
                vec!["watch for stale context".into()],
            ),
            selected_descriptions: vec!["Keep the current change".into()],
            status: crate::context::CommandCapture::new(
                "jj status",
                "Working copy has modifications".into(),
                String::new(),
            ),
            revset_log: crate::context::CommandCapture::new(
                "jj log",
                "abc123|def456|feature/example|Keep the current change".into(),
                String::new(),
            ),
            diff_stats: crate::context::CommandCapture::new(
                "jj diff --stat",
                " file.rs | 3 ++-".into(),
                String::new(),
            ),
            diff: crate::context::CommandCapture::new(
                "jj diff",
                "diff --git a/file.rs b/file.rs\n+added line".into(),
                String::new(),
            ),
        }
    }

    #[test]
    fn prompt_includes_dirty_form_values_and_schema() {
        let mut context = sample_context();
        let mut form = PrForm::new("@", "feature/example", "main@origin");
        form.branch_name.begin_edit();
        form.branch_name.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        ));
        form.title.begin_edit();
        for ch in "Add prompt manifest".chars() {
            form.title.input(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ));
        }
        form.description.begin_edit();
        for ch in "Keep the current behavior".chars() {
            form.description.input(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ));
        }
        context.form = form.clone();

        let build = PromptBuild::new(
            &context,
            &form,
            Some("Prefer concise output."),
            DEFAULT_PROMPT_BYTE_BUDGET,
        );

        assert!(build.prompt.contains("branch name: feature/examplex"));
        assert!(
            build
                .prompt
                .contains("Return strict JSON matching this schema")
        );
        assert!(build.prompt.contains("Do not wrap it in a Markdown fence"));
        assert!(build.prompt.contains("Add prompt manifest"));
        assert_eq!(build.manifest.form_values.title, "Add prompt manifest");
        assert_eq!(build.manifest.selected_revset, "@");
        assert_eq!(build.manifest.base_branch, "main@origin");
    }

    #[test]
    fn prompt_omits_config_values_and_sanitizes_remote_urls() {
        let context = sample_context();
        let form = PrForm::new("@", "feature/example", "main@origin");
        let build = PromptBuild::new(&context, &form, None, DEFAULT_PROMPT_BYTE_BUDGET);

        assert!(!build.prompt.contains("http://localhost:11434"));
        assert!(!build.prompt.contains("qwen2.5-coder:latest"));
        assert!(
            !build
                .prompt
                .contains("git@code.example.com:team/project.git")
        );
        assert!(build.prompt.contains("code.example.com"));
    }

    #[test]
    fn prompt_reports_truncation_and_omissions() {
        let mut context = sample_context();
        context.diff = crate::context::CommandCapture::new(
            "jj diff",
            "diff --git a/file.rs b/file.rs\n".repeat(100),
            String::new(),
        );

        let form = PrForm::new("@", "feature/example", "main@origin");
        let build = PromptBuild::new(&context, &form, None, 1_600);

        assert!(!build.manifest.truncation_warnings.is_empty());
        assert!(
            build
                .manifest
                .included_sections
                .iter()
                .any(|section| section.truncated)
        );
        assert!(!build.manifest.omitted_sections.is_empty());
    }
}
