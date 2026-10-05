//! Stable rendered workflow paths, IDs, and policy spellings.

use velnor_actions_contract::{
    CI_WORKFLOW_PATH, PLAN_JOB_ID as CONTRACT_PLAN_JOB_ID,
    REQUIRED_CONDITION as CONTRACT_REQUIRED_CONDITION,
    REQUIRED_DISPLAY_NAME as CONTRACT_REQUIRED_DISPLAY_NAME,
    REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID,
};

/// Generated workflow path inside the repository.
///
/// Alias of the contract's [`CI_WORKFLOW_PATH`]: the migration plan and
/// emitted tree share one source of truth.
pub const WORKFLOW_PATH: &str = CI_WORKFLOW_PATH;
/// Generated actionlint config path inside the repository.
pub const ACTIONLINT_PATH: &str = ".github/actionlint.yaml";
/// Exact pull-request event types.
pub const EXPECTED_PR_TYPES: &[&str] = &["opened", "synchronize", "reopened", "ready_for_review"];
/// Exact concurrency group expression.
pub const CONCURRENCY_GROUP: &str =
    "velnor-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}";
/// Exact cancel-in-progress expression (PR events only).
pub const CONCURRENCY_CANCEL: &str = "${{ github.event_name == 'pull_request' }}";
/// Final gate job ID (contract alias).
pub const FINAL_JOB_ID: &str = CONTRACT_REQUIRED_JOB_ID;
/// Exact required-check display name (contract alias).
pub const FINAL_DISPLAY_NAME: &str = CONTRACT_REQUIRED_DISPLAY_NAME;
/// Final gate condition (contract alias).
pub const FINAL_CONDITION: &str = CONTRACT_REQUIRED_CONDITION;
/// Planner job ID: the sole matrix producer (contract alias).
pub const PLAN_JOB_ID: &str = CONTRACT_PLAN_JOB_ID;
/// Matrix consumer job ID.
pub const TASK_JOB_ID: &str = "velnor-task";
/// Candidate validation job ID (Velnor policy only).
pub const CANDIDATE_JOB_ID: &str = "candidate";
/// Baseline-publish job ID: runs after the final gate passes.
pub const PUBLISH_JOB_ID: &str = "publish-baseline";
/// Full-SHA Alint pin for the repository-policy `alint` job.
pub const ALINT_USES: &str = "asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb";
/// Pinned Alint binary release tag for the step's `version:` input.
///
/// The action falls back to installing `latest` when `version:` is absent.
pub const ALINT_BINARY_VERSION: &str = "v0.16.1";
