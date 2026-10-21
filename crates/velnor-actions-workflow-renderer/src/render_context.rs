use super::{BTreeMap, RenderError, commands, guard, marker, steps, validator_tools};
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, PLAN_JOB_ID as CONTRACT_PLAN_JOB_ID, PullRequestCachePolicy,
    REQUIRED_CONDITION as CONTRACT_REQUIRED_CONDITION, REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID,
    REQUIRED_VERDICT_DISPLAY_NAME as CONTRACT_REQUIRED_VERDICT_DISPLAY_NAME, ScaleSetSelector,
    ValidatorKind,
};

pub use crate::matrix::{
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV, MatrixSource,
    NOT_AFFECTED_JOBS_OUTPUT, PLAN_ATTEMPT_ENV, PLAN_ATTEMPT_OUTPUT, PLAN_ID_OUTPUT, PLAN_STEP_ID,
    RUN_KEY_OUTPUT,
};
pub use crate::plan_gate::not_affected_gate;
pub use crate::setup::MiseSetup;

/// Generated workflow path inside the repository.
///
/// Alias of the contract's [`CI_WORKFLOW_PATH`]: the migration plan
/// ([`velnor_actions_contract::RequiredCheckMigration`]) and the
/// emitted tree share one source of truth, never retyped mirrors.
pub const WORKFLOW_PATH: &str = CI_WORKFLOW_PATH;
/// Generated jactionlint config path inside the repository.
pub const JACTIONLINT_PATH: &str = velnor_actions_actionlint::JACTIONLINT_CONFIG_PATH;
/// Exact pull-request event types.
pub const EXPECTED_PR_TYPES: &[&str] = &[
    "opened",
    "edited",
    "synchronize",
    "reopened",
    "ready_for_review",
];
/// Exact concurrency group expression.
pub const CONCURRENCY_GROUP: &str =
    "velnor-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}";
/// Exact cancel-in-progress expression (PR events only).
pub const CONCURRENCY_CANCEL: &str = "${{ github.event_name == 'pull_request' }}";
/// Final verdict job ID (contract [`CONTRACT_REQUIRED_JOB_ID`] alias).
pub const FINAL_JOB_ID: &str = CONTRACT_REQUIRED_JOB_ID;
/// Exact verdict display name (contract alias); the protected
/// `Required` check is the gate job (`required_gate`).
pub const FINAL_DISPLAY_NAME: &str = CONTRACT_REQUIRED_VERDICT_DISPLAY_NAME;
/// Final gate condition (contract [`CONTRACT_REQUIRED_CONDITION`] alias).
pub const FINAL_CONDITION: &str = CONTRACT_REQUIRED_CONDITION;

/// Condition attached to report upload steps: they run even when the job
/// fails or the run is cancelled.
pub const UPLOAD_STEP_CONDITION: &str = "always()";
/// Planner job ID: the sole matrix producer (contract alias).
pub const PLAN_JOB_ID: &str = CONTRACT_PLAN_JOB_ID;
/// Matrix consumer job ID.
pub const TASK_JOB_ID: &str = "velnor-task";
/// Candidate validation job ID (Velnor policy only).
pub const CANDIDATE_JOB_ID: &str = "candidate";
/// Full-SHA Alint pin for the repository-policy `alint` job.
pub const ALINT_USES: &str = "asamarts/alint@f39a25a7e061bb8738a87372561d8b9a6a29ca87";

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
    /// Workflow-level Mise silencing from `workflow.mise_quiet` (default
    /// true): `false` omits `MISE_QUIET` from the generated workflow
    /// environment so the wrapper-dispatch trace stays visible.
    pub mise_quiet: bool,
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
            if command.validator == ValidatorKind::Jactionlint {
                return Err(RenderError::BadCommand("jactionlint_not_support".to_owned()));
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
