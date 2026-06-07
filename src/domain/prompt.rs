use super::context::ContextBundle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBuild {
    pub prompt: String,
    pub manifest: PromptManifest,
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

const SYSTEM: &str = "You are a PR drafting assistant. Read the change description, status, log, and diff below, then output a JSON object with exactly two fields:\n  \"title\": one-line imperative summary (≤72 chars, no trailing period)\n  \"description\": markdown body explaining what changed and why (≤500 words)\nRespond with ONLY the JSON object — no markdown fences, no prose before or after.\n\n";

pub fn build_prompt(ctx: &ContextBundle) -> PromptBuild {
    let mut buf = String::new();
    let mut sections = Vec::new();
    buf.push_str(SYSTEM);

    push_section(&mut buf, &mut sections, "Status", &ctx.status);
    push_section(&mut buf, &mut sections, "Log", &ctx.log);
    push_section(&mut buf, &mut sections, "Diff Stats", &ctx.diff_stats);
    push_section(&mut buf, &mut sections, "Diff", &ctx.diff);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ContextBundle {
        ContextBundle {
            revset: "abcd".into(),
            status: "Working copy : abcd Test".into(),
            log: "abcd Test desc".into(),
            diff_stats: "1 file changed".into(),
            diff: "@@ -1 +1 @@\n-old\n+new".into(),
            diff_truncated: false,
        }
    }

    #[test]
    fn build_prompt_lists_all_four_sections() {
        let prompt = build_prompt(&sample());
        let names: Vec<&str> = prompt.manifest.sections.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["Status", "Log", "Diff Stats", "Diff"]);
    }

    #[test]
    fn build_prompt_total_bytes_matches_prompt_len() {
        let prompt = build_prompt(&sample());
        assert_eq!(prompt.manifest.total_bytes, prompt.prompt.len());
    }

    #[test]
    fn empty_section_renders_placeholder() {
        let mut ctx = sample();
        ctx.diff = String::new();
        let prompt = build_prompt(&ctx);
        assert!(prompt.prompt.contains("## Diff\n(empty)"));
    }
}
