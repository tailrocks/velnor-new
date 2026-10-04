//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates: exact triggers, concurrency, single runner label,
//! consumer support rejection, and candidate-never-plans invariants.
//! The strict entrypoint additionally mandates Mise setup, staged
//! helpers, and the plan anchor. The basic entrypoint accepts read-only jobs
//! without phased planning or producer authority.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CI_WORKFLOW_PATH, Job, PLAN_JOB_ID as CONTRACT_PLAN_JOB_ID,
    REQUIRED_CONDITION as CONTRACT_REQUIRED_CONDITION,
    REQUIRED_DISPLAY_NAME as CONTRACT_REQUIRED_DISPLAY_NAME,
    REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID, ValidatorKind, VelnorSupportWorkflow, WorkflowIr,
    WorkflowPolicy,
};

use crate::{
    RenderError, cache_p08, closure, commands, document, final_steps, guard, marker, matrix,
    preseed_closure, steps, support, yaml::render_yaml,
};

#[path = "render_admission.rs"]
mod admission;
use admission::merged_jobs;

pub use crate::matrix::{
    COVERED_TASKS_OUTPUT, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
    MatrixSource, PLAN_ID_OUTPUT, PLAN_STEP_ID, RUN_KEY_OUTPUT,
};
pub use crate::setup::MiseSetup;

/// Materialize support jobs before executable producer cohorts are derived.
/// # Errors
/// Rejects inconsistent validator or candidate policy inputs.
pub fn merge_support_jobs(
    jobs: &mut BTreeMap<String, Job>,
    workflow: Option<&VelnorSupportWorkflow>,
    context: &RenderContext,
) -> Result<(), RenderError> {
    support::merge_support_jobs(jobs, workflow, context)
}

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
    /// Exact generator version for the marker and staged path.
    pub generator_version: String,
    /// Single literal versioned Ubuntu label every job must use.
    pub runs_on: String,
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
    /// Caller-validated env for plan-job helper consumers: the freshness
    /// step and the `plan-v1` internal step run the helper, whose
    /// locked/offline qualification reads the Cargo home the Fetch step
    /// populated. Opaque to the renderer (attached verbatim); the
    /// orchestrator supplies the same validated constructor Fetch uses
    /// so fetch and consumers match by construction (run 36754512444
    /// failed `generate` on ambient homes after plan fetch moved to
    /// owned homes).
    pub plan_consumer_env: BTreeMap<String, String>,
    /// Exact source and invocation records supplied by compiled operation owners.
    pub source_helpers: Vec<velnor_actions_contract::CompiledSourceHelper>,
    /// Generation-only approvals from the compiled native deployment factory.
    pub native_pages_approvals: Vec<crate::pages_approval::NativePagesApproval>,
    /// Exact generation-only native public attestation workflow approvals.
    pub native_publish_approvals: Vec<crate::native_publish_approval::NativePublishApproval>,
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
}

/// Fixed candidate-job vectors (Velnor policy only).
#[derive(Debug, Clone)]
pub struct CandidateSpec {
    /// Fixed candidate-build argv.
    pub build: Vec<String>,
    /// Fixed candidate-qualification argv.
    pub qualify: Vec<String>,
}

pub use crate::tree::{RenderedFile, RenderedSymlink, RenderedTree};
pub use velnor_actions_contract::{AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET};

impl RenderContext {
    /// Validate every context scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        crate::source_helper::validate_registry(&self.source_helpers, &self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        guard::validate_staged_binary(&self.staged_binary, &self.generator_version)?;
        guard::validate_request_dir(&self.request_dir)?;
        steps::checkout_step(&self.checkout_uses).map(|_| ())?;
        for command in &self.validator_commands {
            if command.validator == ValidatorKind::Actionlint {
                return Err(RenderError::BadCommand("actionlint_not_support".to_owned()));
            }
            if command.name.trim().is_empty() {
                return Err(RenderError::BadCommand("empty_validator_name".to_owned()));
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

/// Render one workflow document from IR under a policy gate.
///
/// Inserts the mandated closures (plan, request, task, final) shared by
/// both entrypoints; use [`render_workflow_ir_strict`] for the full
/// fail-closed pass (Mise setup plus staged-helper gates).
///
/// # Errors
///
/// Returns [`RenderError`] for invalid context, IR, policy, or steps.
pub fn render_workflow_ir(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<String, RenderError> {
    let mut jobs = merged_jobs(ir, policy, support, ctx)?;
    admission::require_nonstrict_readonly(&jobs)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    render_merged(ir, &jobs, ctx, false)
}

/// Strict render: setup insertion plus staged-helper and anchor gates.
///
/// Every `mise`-invoking job (plus plan/task unconditionally) gains a
/// preceding pinned setup step; internal steps without a preceding
/// Acquire step and anchorless plan jobs fail closed. In pre-seed mode
/// the fixed pre-seed stage step stages instead, and the build-once
/// artifact closure is enforced. Pins arrive via `mise`; the renderer
/// never invents them.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, or steps.
pub fn render_workflow_ir_strict(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<String, RenderError> {
    Ok(render_workflow_ir_strict_with_jobs(ir, policy, support, ctx, mise)?.0)
}

/// Strictly render and return the exact internally finalized jobs used by YAML.
/// # Errors
/// Rejects the same invalid pins, context, workflow, and staging as strict rendering.
pub fn render_workflow_ir_strict_with_jobs(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<(String, BTreeMap<String, Job>), RenderError> {
    let jobs = finalize_jobs(ir, policy, support, ctx, mise)?;
    let yaml = render_merged(ir, &jobs, ctx, true)?;
    Ok((yaml, jobs))
}

/// Finalize jobs exactly as `generate` writes them: policy merge, setup
/// insertion, then every closure and the final fan-in.
///
/// `plan` lists these jobs (validators included) so its job table,
/// step counts, and action pins match the written YAML by construction
/// instead of echoing the pre-merge IR.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, or steps.
pub fn finalize_jobs(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<BTreeMap<String, Job>, RenderError> {
    mise.validate()?;
    let mut jobs = merged_jobs(ir, policy, support, ctx)?;
    for (id, job) in &mut jobs {
        if id == "verification-observer" {
            continue;
        }
        if cache_p08::tool_roles::validate_tool_producer(job, mise, &ctx.source_helpers)? {
            continue;
        }
        let always = id == PLAN_JOB_ID || id == TASK_JOB_ID;
        let target = velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
            .ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
            })?;
        cache_p08::ensure_setup_p08(id, job, mise, always, target, &ctx.source_helpers)?;
        if crate::early_plan::has_early_plan(job) {
            cache_p08::phases::validate_prefix(job, mise, target, ctx)?;
        }
        cache_p08::check_no_rust_cache_with_mbx(id, job)?;
        cache_p08::check_mbx_before_fetch(id, job)?;
        closure::check_internal_staged(id, job, ctx.preseed)?;
    }
    // Only structurally admitted pure jobs may export executable snapshots.
    cache_p08::validate_tool_consumers(&jobs, mise, &ctx.source_helpers)?;
    closure::check_plan_anchor(&jobs)?;
    preseed_closure::check_preseed_closure(&jobs, ctx.preseed)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    Ok(jobs)
}

/// Sorted unique `uses:` refs across every action step (plan display).
#[must_use]
pub fn action_pins(jobs: &BTreeMap<String, Job>) -> Vec<String> {
    let mut pins = std::collections::BTreeSet::new();
    for job in jobs.values() {
        for step in &job.steps {
            if let velnor_actions_contract::StepKind::Action { uses, .. } = &step.kind {
                pins.insert(uses.clone());
            }
        }
    }
    pins.into_iter().collect()
}

/// Emit matrix strategy plus the quoted, marked workflow text.
fn render_merged(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    cache_writers_admitted: bool,
) -> Result<String, RenderError> {
    let matrix = matrix::task_matrix_of(jobs)?;
    let caps = matrix::crate_job_caps(jobs)?;
    let jobs = if matrix.is_some() || !caps.is_empty() {
        matrix::scrub_matrix_marker(jobs)
    } else {
        jobs.clone()
    };
    let mut document = document::workflow_to_yaml(ir, &jobs, ctx, cache_writers_admitted)?;
    if let Some((source, max_parallel)) = &matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    crate::plan_fallback::attach(&mut document)?;
    matrix::attach_crate_job_caps(&mut document, &caps)?;
    matrix::insert_publish_step_id(&mut document)?;
    let document = crate::yaml::quote_run_values_in_yaml(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}
