pub mod bookmark;
pub mod context;
pub mod execute;
pub mod jj_mutate;
pub mod llm;
pub mod probe;
pub(crate) mod process;
pub mod prompt;
pub mod stack;
pub mod status_store;

pub use bookmark::slugify;
pub use context::{
    ChangeContext, ContextBundle, ContextJob, ContextResult, DiffContext, STACK_RANGE_DIFF_FLOOR,
    StackContextJob, StackContextResult, divide_budget,
};
pub use execute::{ExecutePrJob, ExecuteResult, ExecuteStep, StackPushJob, StackPushResult};
pub use jj_mutate::{JjMutateJob, JjMutateResult, JjOp, JjOpKind};
pub use llm::{
    CacheHealth, GeneratedDraft, LlmGenerateJob, LlmResult, StackLlmResult, StackPrLlmJob,
    fallback_stack_draft, parse_stack_drafts,
};
pub use probe::{
    BackendHealth, BackendHealthProbe, BaseBookmark, BaseBookmarks, BaseBookmarksProbe, LlmHealth,
    RemoteInfo, RepoOptions, RepoOptionsProbe, RevsetProbe, RevsetStats, RevsetStatsProbe,
    RevsetSummary, Revsets, StackExistingPr, StackExistingPrs, StackExistingPrsProbe,
    StackPushPrecheck, StackPushPrecheckJob, TeaAuthProbe, TeaAuthStatus, ToolStatus, VersionKind,
    VersionProbe, VersionResult, WorkspaceInfo, WorkspaceProbe,
};
pub use prompt::{
    PromptBuild, PromptForm, PromptManifest, PromptSection, StackPrefix, build_prompt,
    build_stack_prefix, stack_pr_suffix,
};
pub use stack::{
    BulkPhase, PrStatus, StackDraft, StackIntent, StackPlan, StackPlanItem, StackPrInput,
    StackSelection, annotate_blockers, annotate_order_blockers, derive_stack_ranges,
    mark_created_from_existing_prs,
};
pub use status_store::StatusStore;
