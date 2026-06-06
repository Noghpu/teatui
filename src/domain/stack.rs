use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::domain::execute::ExecuteStep;
use crate::domain::forge::StackExistingPr;
use crate::domain::probe::RevsetSummary;

// ============================== Domain Types ================================

/// Overall guidance from the Form when in bulk mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackIntent {
    pub title: String,
    pub description: String,
    pub branch: String,
}

/// Input for one PR in a stacked chain.
///
/// - `base`: PR 1 = form base; PR k = previous bookmark (filled at plan build).
/// - `included_change_ids`: all change ids in this PR's range (oldest-to-newest
///   within the range), including unselected gap changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPrInput {
    pub index: usize,
    pub base: String,
    pub head: String,
    pub included_change_ids: Vec<String>,
    pub subject: String,
}

/// Assembled at `G`: drives context collection and prompt building.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackSelection {
    pub items: Vec<StackPrInput>,
    pub intent: StackIntent,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
}

/// One parsed LLM row in the batch response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackDraft {
    pub index: usize,
    pub pr_type: String,
    pub branch_slug: String,
    pub title: String,
    pub description: String,
}

/// Per-PR push status in the review modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrStatus {
    Pending,
    Bookmarked,
    Pushed,
    Created { url: String },
    Failed { step: ExecuteStep, message: String },
}

/// One item in the final stack plan (post-LLM, pre-push).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPlanItem {
    pub input: StackPrInput,
    pub bookmark: String,
    pub title: String,
    pub description: String,
    pub status: PrStatus,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
}

/// Full stack plan presented in the review modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPlan {
    pub items: Vec<StackPlanItem>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
    pub intent: StackIntent,
}

// ============================== Blockers ====================================

/// Annotate each plan item with bookmark / existing-PR blockers.
///
/// Checks are intentionally pure and side-effect free beyond rewriting the
/// `blockers` vector on each item. This is the shared logic used when the plan
/// enters review and later when an item is re-checked before push.
pub fn annotate_blockers(
    plan: &mut StackPlan,
    local_bookmarks: &[String],
    existing_prs: &[StackExistingPr],
) {
    let local_bookmarks: HashSet<&str> = local_bookmarks.iter().map(String::as_str).collect();
    let existing_prs_by_head: HashMap<&str, &StackExistingPr> = existing_prs
        .iter()
        .map(|pr| (pr.head_branch.as_str(), pr))
        .collect();
    let mut seen_bookmarks: HashMap<String, usize> = HashMap::new();

    for (index, item) in plan.items.iter_mut().enumerate() {
        item.blockers.clear();
        let bookmark = item.bookmark.clone();

        if let Some(previous_index) = seen_bookmarks.insert(bookmark.clone(), index) {
            item.blockers.push(format!(
                "duplicate generated bookmark {bookmark}; also used by PR {}",
                previous_index + 1
            ));
        }

        if matches!(item.status, PrStatus::Pending) {
            if let Some(existing_pr) = existing_prs_by_head.get(bookmark.as_str()) {
                item.blockers
                    .push(format_existing_pr_blocker(&bookmark, existing_pr));
                continue;
            }

            if local_bookmarks.contains(bookmark.as_str()) {
                item.blockers
                    .push(format!("bookmark {bookmark} already exists"));
            }
        }
    }
}

/// Mark plan rows as already created when the live PR list reports the
/// generated bookmark as an existing PR head.
pub fn mark_created_from_existing_prs(plan: &mut StackPlan, existing_prs: &[StackExistingPr]) {
    let existing_prs_by_head: HashMap<&str, &StackExistingPr> = existing_prs
        .iter()
        .map(|pr| (pr.head_branch.as_str(), pr))
        .collect();

    for item in &mut plan.items {
        if matches!(item.status, PrStatus::Created { .. }) {
            continue;
        }
        let Some(existing_pr) = existing_prs_by_head.get(item.bookmark.as_str()) else {
            continue;
        };
        let Some(url) = existing_pr
            .url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        item.status = PrStatus::Created {
            url: url.to_string(),
        };
    }
}

/// Annotate order blockers for later PRs in a push sequence.
///
/// A later item may only be pushed once every earlier item is already
/// `Created`. This is separate from bookmark / existing-PR blockers so the
/// review modal can show which rows are simply waiting on earlier PRs versus
/// which rows conflict with current repo state.
pub fn annotate_order_blockers(plan: &mut StackPlan) {
    for index in 0..plan.items.len() {
        if index == 0 {
            continue;
        }
        let prev = &plan.items[index - 1];
        if !matches!(prev.status, PrStatus::Created { .. }) {
            plan.items[index]
                .blockers
                .push(format!("wait for PR {} to be created first", index));
        }
    }
}

fn format_existing_pr_blocker(bookmark: &str, existing_pr: &StackExistingPr) -> String {
    let mut details = existing_pr.state.trim().to_string();
    if details.is_empty() {
        details = "existing".into();
    }
    let mut message = format!("existing PR for {bookmark} ({details})");
    if let Some(url) = existing_pr
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        message.push_str(&format!(": {url}"));
    }
    message
}

// ============================== BulkPhase ===================================

/// Phase of the bulk stacked-PR flow, held by `GenerateState`. The bulk modal
/// is open whenever this is not `Idle`, and while open it captures all keys the
/// same way the jj-op and picker modals do.
#[derive(Debug, Clone, Default)]
pub enum BulkPhase {
    /// No bulk flow in progress; the main three-pane screen is interactive.
    #[default]
    Idle,
    /// Collecting per-range context (modal shows a loading state; `Esc` cancels).
    Collecting,
    /// Sequentially generating one LLM draft per selected PR. The prefix is
    /// shared across row jobs so the backend can reuse its KV cache.
    Generating {
        prefix: Arc<str>,
        inputs: Vec<StackPrInput>,
        intent: StackIntent,
        labels: Vec<String>,
        assignees: Vec<String>,
        milestone: String,
        drafts: Vec<Option<StackDraft>>,
        warnings: Vec<Vec<String>>,
        next: usize,
        total: usize,
    },
    /// Two-pane review: PR list plus a per-PR editor, with push controls.
    Review {
        plan: StackPlan,
        /// Highlighted PR in the list; drives the right-hand editor.
        cursor: usize,
        /// Index of the PR whose push job is in flight, if any. While this is
        /// `Some`, mutating actions are disabled but list navigation stays
        /// responsive.
        pushing: Option<usize>,
        /// Whether the active push is walking the whole stack (`P`) rather than
        /// pushing a single PR (`p`).
        push_all: bool,
    },
    /// Context collection or generation failed (modal shows the error; `Esc` closes).
    Failed { message: String },
}

// ============================== Range derivation ============================

/// Derive oldest-to-newest `StackPrInput` slices from the selected heads.
///
/// `revsets` is the Changes-pane list in display order (newest-first).
/// `selected_ids` may arrive in any order; the function re-orders them by
/// their position in `revsets` so the output is always oldest-to-newest.
/// Stale ids (not present in `revsets`) are silently dropped.
///
/// Gap changes (revset entries between two selected heads) fold into the
/// later (newer) PR's `included_change_ids`. The oldest selected head's PR
/// range starts right after `base`.
///
/// Returns an empty `Vec` when no valid selection can be derived.
pub fn derive_stack_ranges(
    revsets: &[RevsetSummary],
    selected_ids: &[String],
    base: &str,
) -> Vec<StackPrInput> {
    // Build a position map: change_id -> index in the newest-first revsets
    // list (index 0 = newest, len-1 = oldest).
    let position_of: std::collections::HashMap<&str, usize> = revsets
        .iter()
        .enumerate()
        .map(|(i, r)| (r.change_id.as_str(), i))
        .collect();

    // Collect valid selected ids, keeping only those present in revsets.
    let mut sorted_positions: Vec<usize> = selected_ids
        .iter()
        .filter_map(|id| position_of.get(id.as_str()).copied())
        .collect();

    if sorted_positions.is_empty() {
        return Vec::new();
    }

    // Order oldest-to-newest. The list is newest-first (index 0 = newest), so
    // the oldest head has the highest index: sort positions descending.
    sorted_positions.sort_unstable_by(|a, b| b.cmp(a));

    let mut result: Vec<StackPrInput> = Vec::with_capacity(sorted_positions.len());

    for (pr_index, &head_pos) in sorted_positions.iter().enumerate() {
        let head_id = &revsets[head_pos].change_id;
        let subject = revsets[head_pos].description.clone();

        // PR 0's base is the form base; PR k's base is the previous selected
        // head's change_id. The change_id is a placeholder the plan-build step
        // (a later slice) resolves to the previous PR's bookmark.
        let pr_base = if pr_index == 0 {
            base.to_string()
        } else {
            result[pr_index - 1].head.clone()
        };

        // This PR covers `(prev_head, this_head]` in jj terms, with gap changes
        // folding into the later PR. In the newest-first list that is the slice
        // `[head_pos..upper_bound)`: down to (and including) this head, up to but
        // excluding the previous selected head. PR 0 has no previous head, so it
        // reaches the end of the list (the oldest change).
        let upper_bound = if pr_index == 0 {
            revsets.len()
        } else {
            sorted_positions[pr_index - 1]
        };

        // The slice is newest-first; reverse it for oldest-to-newest order.
        let included: Vec<String> = revsets[head_pos..upper_bound]
            .iter()
            .rev()
            .map(|r| r.change_id.clone())
            .collect();

        result.push(StackPrInput {
            index: pr_index,
            base: pr_base,
            head: head_id.clone(),
            included_change_ids: included,
            subject,
        });
    }

    result
}

// ============================== Tests =======================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::probe::RevsetSummary;

    fn revset(change_id: &str, description: &str) -> RevsetSummary {
        RevsetSummary {
            label: format!("trunk()..{change_id}"),
            change_id: change_id.into(),
            commit_id: format!("{change_id}-commit"),
            bookmarks: Vec::new(),
            description: description.into(),
            description_body: String::new(),
            author: String::new(),
            stats: String::new(),
            commit_count: 1,
            commit_ids: vec![format!("{change_id}-commit")],
            change_ids: vec![change_id.into()],
            recent_log: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Simulate a newest-first revsets list:  e (newest) .. a (oldest).
    fn five_revsets() -> Vec<RevsetSummary> {
        vec![
            revset("e", "Change E"),
            revset("d", "Change D"),
            revset("c", "Change C"),
            revset("b", "Change B"),
            revset("a", "Change A"),
        ]
    }

    #[test]
    fn empty_selection_returns_empty() {
        let result = derive_stack_ranges(&five_revsets(), &[], "main");
        assert!(result.is_empty());
    }

    #[test]
    fn stale_ids_are_ignored() {
        let result =
            derive_stack_ranges(&five_revsets(), &["x".to_string(), "z".to_string()], "main");
        assert!(result.is_empty());
    }

    #[test]
    fn single_head_no_gaps() {
        let result = derive_stack_ranges(&five_revsets(), &["c".to_string()], "main");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].index, 0);
        assert_eq!(result[0].base, "main");
        assert_eq!(result[0].head, "c");
        // includes c, b, a (oldest-to-newest within the range: a, b, c)
        assert_eq!(result[0].included_change_ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn two_heads_no_gaps() {
        // Select a and c (contiguous)
        let result =
            derive_stack_ranges(&five_revsets(), &["a".to_string(), "c".to_string()], "main");
        assert_eq!(result.len(), 2);

        // PR 0 (oldest first = a)
        assert_eq!(result[0].head, "a");
        assert_eq!(result[0].base, "main");
        assert_eq!(result[0].included_change_ids, vec!["a"]);

        // PR 1 (next = c, with b as gap)
        assert_eq!(result[1].head, "c");
        assert_eq!(result[1].base, "a"); // previous head's change_id
        assert_eq!(result[1].included_change_ids, vec!["b", "c"]);
    }

    #[test]
    fn gaps_fold_into_later_pr() {
        // Select a and e; b, c, d are gaps that fold into e's PR
        let result =
            derive_stack_ranges(&five_revsets(), &["a".to_string(), "e".to_string()], "main");
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].head, "a");
        assert_eq!(result[0].included_change_ids, vec!["a"]);

        assert_eq!(result[1].head, "e");
        assert_eq!(result[1].base, "a");
        assert_eq!(result[1].included_change_ids, vec!["b", "c", "d", "e"]);
    }

    #[test]
    fn selection_order_does_not_affect_output() {
        let forward = derive_stack_ranges(
            &five_revsets(),
            &["a".to_string(), "c".to_string(), "e".to_string()],
            "main",
        );
        let backward = derive_stack_ranges(
            &five_revsets(),
            &["e".to_string(), "c".to_string(), "a".to_string()],
            "main",
        );
        assert_eq!(forward, backward);
    }

    #[test]
    fn three_heads_chains_bases() {
        let result = derive_stack_ranges(
            &five_revsets(),
            &["a".to_string(), "c".to_string(), "e".to_string()],
            "main",
        );
        assert_eq!(result.len(), 3);

        assert_eq!(result[0].head, "a");
        assert_eq!(result[0].base, "main");
        assert_eq!(result[0].included_change_ids, vec!["a"]);

        assert_eq!(result[1].head, "c");
        assert_eq!(result[1].base, "a");
        assert_eq!(result[1].included_change_ids, vec!["b", "c"]);

        assert_eq!(result[2].head, "e");
        assert_eq!(result[2].base, "c");
        assert_eq!(result[2].included_change_ids, vec!["d", "e"]);
    }

    #[test]
    fn stale_ids_mixed_with_valid_are_silently_dropped() {
        let result = derive_stack_ranges(
            &five_revsets(),
            &["a".to_string(), "stale".to_string(), "c".to_string()],
            "main",
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].head, "a");
        assert_eq!(result[1].head, "c");
    }

    #[test]
    fn single_head_newest_change_no_gaps() {
        // Select only the newest change: e
        let result = derive_stack_ranges(&five_revsets(), &["e".to_string()], "main");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].head, "e");
        assert_eq!(result[0].base, "main");
        // All changes below it fold in: a, b, c, d, e
        assert_eq!(result[0].included_change_ids, vec!["a", "b", "c", "d", "e"]);
    }

    fn plan_item(bookmark: &str) -> StackPlanItem {
        StackPlanItem {
            input: StackPrInput {
                index: 0,
                base: "main".into(),
                head: "head".into(),
                included_change_ids: vec!["head".into()],
                subject: "subject".into(),
            },
            bookmark: bookmark.into(),
            title: "title".into(),
            description: "description".into(),
            status: PrStatus::Pending,
            warnings: Vec::new(),
            blockers: Vec::new(),
        }
    }

    #[test]
    fn duplicate_generated_bookmarks_block_later_items_only() {
        let mut plan = StackPlan {
            items: vec![plan_item("pr/feat/foo"), plan_item("pr/feat/foo")],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        annotate_blockers(&mut plan, &[], &[]);

        assert!(plan.items[0].blockers.is_empty());
        assert_eq!(plan.items[1].blockers.len(), 1);
        assert!(plan.items[1].blockers[0].contains("duplicate generated bookmark pr/feat/foo"));
    }

    #[test]
    fn local_and_remote_bookmarks_block() {
        let mut plan = StackPlan {
            items: vec![plan_item("pr/feat/foo"), plan_item("pr/fix/bar")],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        annotate_blockers(
            &mut plan,
            &["pr/feat/foo".to_string(), "pr/fix/bar".to_string()],
            &[],
        );

        assert_eq!(
            plan.items[0].blockers,
            vec!["bookmark pr/feat/foo already exists"]
        );
        assert_eq!(
            plan.items[1].blockers,
            vec!["bookmark pr/fix/bar already exists"]
        );
    }

    #[test]
    fn existing_prs_take_precedence_over_bookmark_collisions() {
        let mut plan = StackPlan {
            items: vec![plan_item("pr/feat/foo")],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        annotate_blockers(
            &mut plan,
            &["pr/feat/foo".to_string()],
            &[StackExistingPr {
                head_branch: "pr/feat/foo".into(),
                state: "open".into(),
                url: Some("https://example.com/pulls/17".into()),
            }],
        );

        assert_eq!(plan.items[0].blockers.len(), 1);
        assert!(plan.items[0].blockers[0].contains("existing PR for pr/feat/foo"));
        assert!(plan.items[0].blockers[0].contains("open"));
        assert!(plan.items[0].blockers[0].contains("https://example.com/pulls/17"));
    }

    #[test]
    fn clean_plan_has_no_blockers() {
        let mut plan = StackPlan {
            items: vec![plan_item("pr/feat/foo")],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        annotate_blockers(&mut plan, &[], &[]);
        assert!(plan.items[0].blockers.is_empty());
    }

    #[test]
    fn completed_items_do_not_self_block_on_live_bookmarks() {
        let mut item = plan_item("pr/feat/foo");
        item.status = PrStatus::Created {
            url: "https://example.com/pulls/1".into(),
        };
        let mut plan = StackPlan {
            items: vec![item],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        annotate_blockers(&mut plan, &["pr/feat/foo".to_string()], &[]);
        assert!(plan.items[0].blockers.is_empty());
    }

    #[test]
    fn existing_pr_with_url_marks_item_created_for_resume() {
        let mut plan = StackPlan {
            items: vec![plan_item("pr/feat/foo")],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        mark_created_from_existing_prs(
            &mut plan,
            &[StackExistingPr {
                head_branch: "pr/feat/foo".into(),
                state: "open".into(),
                url: Some("https://example.com/pulls/17".into()),
            }],
        );

        assert_eq!(
            plan.items[0].status,
            PrStatus::Created {
                url: "https://example.com/pulls/17".into()
            }
        );
    }

    #[test]
    fn later_items_get_order_blockers_until_previous_created() {
        let mut plan = StackPlan {
            items: vec![plan_item("pr/feat/foo"), plan_item("pr/fix/bar")],
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: String::new(),
            intent: StackIntent {
                title: String::new(),
                description: String::new(),
                branch: String::new(),
            },
        };

        annotate_order_blockers(&mut plan);

        assert!(plan.items[0].blockers.is_empty());
        assert_eq!(
            plan.items[1].blockers,
            vec!["wait for PR 1 to be created first"]
        );

        plan.items[0].status = PrStatus::Created {
            url: "https://example.com/pulls/1".into(),
        };
        plan.items[1].blockers.clear();
        annotate_order_blockers(&mut plan);

        assert!(plan.items[1].blockers.is_empty());
    }
}
