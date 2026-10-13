use super::{
    BTreeMap, CONTRACT_PLAN_JOB_ID, RenderError, commands, guard, marker, steps, validator_tools,
};
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, PullRequestCachePolicy, REQUIRED_CONDITION as CONTRACT_REQUIRED_CONDITION,
    REQUIRED_DISPLAY_NAME as CONTRACT_REQUIRED_DISPLAY_NAME,
    REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID, ScaleSetSelector, ValidatorKind,
};

pub use crate::matrix::{
    COVERED_TASKS_OUTPUT, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
    MatrixSource, PLAN_ID_OUTPUT, PLAN_STEP_ID, RUN_KEY_OUTPUT,
};
pub use crate::setup::MiseSetup;

/// Generated workflow path inside the repository.
///
/// Alias of the contract's [`CI_WORKFLOW_PATH`]: the migration plan
/// ([`velnor_actions_contract::RequiredCheckMigration`]) and the
/// emitted tree share one source of truth, never retyped mirrors.
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
/// Final gate job ID (contract [`CONTRACT_REQUIRED_JOB_ID`] alias).
pub const FINAL_JOB_ID: &str = CONTRACT_REQUIRED_JOB_ID;
/// Exact required-check display name (contract alias).
pub const FINAL_DISPLAY_NAME: &str = CONTRACT_REQUIRED_DISPLAY_NAME;
/// Final gate condition (contract [`CONTRACT_REQUIRED_CONDITION`] alias).
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
pub const ALINT_USES: &str = "asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81";
/// Pinned Alint binary release tag for the step's `version:` input.
///
/// Per the action's `action.yml`, a SHA-pinned `uses:` falls back to
/// installing `latest` unless `version:` is set — a floating binary. Mirror of
/// `ALINT_ACTION_VERSION` (`velnor-actions-actionlint`, same qualified
/// release); the renderer cannot depend on that crate, so
/// `scripts/check-freshness.sh` pins this mirror to the reviewed
/// `asamarts/alint` inventory row instead of trusting the duplication.
pub const ALINT_BINARY_VERSION: &str = "v0.17.0";

/// Caller-supplied validated scalars the IR cannot carry.
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// Exact generator version for generated-file markers and runtime identity.
    pub generator_version: String,
    /// Exact helper release selected by the consumer's validated release manifest.
    ///
    /// This can differ from `generator_version` while rendering a newer
    /// generator against an older pinned consumer. Task report wrappers and
    /// the staged helper path use this version; generated-file markers do not.
    pub report_helper_version: String,
    /// Single literal versioned Ubuntu label every job must use.
    pub runs_on: String,
    /// Scale Set selected by the validated execution profile, when present.
    /// Typed task steps can use only this exact Linux/amd64 selector.
    pub scale_set_selector: Option<ScaleSetSelector>,
    /// Digest-verified staged binary under runner temp.
    pub staged_binary: String,
    /// Internal request directory under runner temp.
    pub request_dir: String,
    /// Pinned `actions/checkout` ref for rendered support jobs.
    pub checkout_uses: String,
    /// Fixed shell steps for repository validator jobs (P05-6: no umbrella).
    pub validator_commands: Vec<ValidatorCommand>,
    /// Fixed vectors for the `candidate` job, when enabled.
    pub candidate: Option<CandidateSpec>,
    /// Pre-seed mode: Velnor policy without a bootstrap lock (trust-on-
    /// review). Accepts fixed pre-seed staging for internal steps and
    /// requires the build-once artifact closure; never set for consumers.
    pub preseed: bool,
    /// One sorted, variant-dispatched graph of explicitly declared workflow tasks.
    pub workflow_tasks: Vec<crate::verification_jobs::WorkflowTaskPolicy>,
    /// Pull-request cache writes; scoped writes require explicit same-repository opt-in.
    pub pull_request_cache_policy: PullRequestCachePolicy,
    /// Caller-validated env for plan-job helper consumers: the freshness
    /// step and the `plan-v1` internal step run the helper, whose
    /// locked/offline qualification reads the Cargo home the Fetch step
    /// populated. Opaque to the renderer (attached verbatim); the
    /// orchestrator supplies the same validated constructor Fetch uses
    /// so fetch and consumers match by construction (run 36754512444
    /// failed `generate` on ambient homes after plan fetch moved to
    /// owned homes).
    pub plan_consumer_env: BTreeMap<String, String>,
}

/// One fixed validator-job shell step: kind plus display name plus argv.
#[derive(Debug, Clone)]
pub struct ValidatorCommand {
    /// Repository validator owning this step's job.
    pub validator: ValidatorKind,
    /// Step display name.
    pub name: String,
    /// Fixed argument vector.
    pub argv: Vec<String>,
    /// Explicit pinned-tool installation argv executed before `argv`.
    pub prepare_argv: Vec<String>,
}

/// Fixed candidate-job vectors (Velnor policy only).
#[derive(Debug, Clone)]
pub struct CandidateSpec {
    /// Fixed candidate-build argv.
    pub build: Vec<String>,
    /// Fixed candidate-qualification argv.
    pub qualify: Vec<String>,
}

pub use crate::lane_share::RenderedWorkflow;
pub use crate::tree::{RenderedFile, RenderedSymlink, RenderedTree};
pub use velnor_actions_contract::{AGENTS_MD_PATH, CLAUDE_MD_PATH};

impl RenderContext {
    /// Validate every context scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        marker::validate_version(&self.report_helper_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        guard::validate_staged_binary(&self.staged_binary, &self.report_helper_version)?;
        guard::validate_request_dir(&self.request_dir)?;
        steps::checkout_step(&self.checkout_uses).map(|_| ())?;
        for command in &self.validator_commands {
            if command.validator == ValidatorKind::Actionlint {
                return Err(RenderError::BadCommand("actionlint_not_support".to_owned()));
            }
            if command.name.trim().is_empty() {
                return Err(RenderError::BadCommand("empty_validator_name".to_owned()));
            }
            validator_tools::validate_validator_tool_closure(command)?;
            if !command.prepare_argv.is_empty() {
                commands::validate_command_argv(&command.prepare_argv)?;
            }
            commands::validate_command_argv(&command.argv)?;
        }
        if let Some(candidate) = &self.candidate {
            commands::validate_command_argv(&candidate.build)?;
            commands::validate_command_argv(&candidate.qualify)?;
        }
        Ok(())
    }
}
