//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates: exact triggers, concurrency, single runner label,
//! consumer support rejection, and candidate-never-plans invariants.
//! The strict entrypoint additionally mandates Mise setup, staged
//! helpers, and the plan anchor; the legacy entrypoint preserves the
//! previous contract for in-flight callers.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CI_WORKFLOW_PATH, Job, PLAN_JOB_ID as CONTRACT_PLAN_JOB_ID, PullRequestCachePolicy,
    REQUIRED_CONDITION as CONTRACT_REQUIRED_CONDITION,
    REQUIRED_DISPLAY_NAME as CONTRACT_REQUIRED_DISPLAY_NAME,
    REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID, ValidatorKind, VelnorSupportWorkflow, WorkflowIr,
    WorkflowPolicy,
};

use crate::{
    RenderError, cache_p08, closure, commands, document, final_steps, guard, marker, matrix, msrv,
    preseed_closure, steps, support, workflow_policy, yaml::render_yaml,
};

#[path = "render_action_pins.rs"]
mod action_pins_impl;
pub use action_pins_impl::action_pins;

#[path = "validator_tools.rs"]
mod validator_tools;

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
    Ok(render_workflow_parts(ir, policy, support, ctx)?.yaml)
}

fn render_workflow_parts(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<RenderedWorkflow, RenderError> {
    let mut jobs = merged_jobs(ir, policy, support, ctx)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    validate_final_jobs(ir, &jobs)?;
    render_merged(ir, &jobs, ctx)
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
    Ok(render_workflow_ir_strict_shared(ir, policy, support, ctx, mise)?.yaml)
}

/// Strict render plus the composite actions duplicated lanes call.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid pins, context, IR, policy,
/// missing setup/staging, steps, or a lane pair whose bodies differ.
pub fn render_workflow_ir_strict_shared(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    mise: &MiseSetup,
) -> Result<RenderedWorkflow, RenderError> {
    let jobs = finalize_jobs(ir, policy, support, ctx, mise)?;
    render_merged(ir, &jobs, ctx)
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
        if ctx.workflow_tasks.iter().any(|task| task.owns_job_id(id)) {
            closure::check_internal_staged(id, job, ctx.preseed)?;
            continue;
        }
        let always = id == PLAN_JOB_ID || id == TASK_JOB_ID || job.check_runner.is_some();
        let target = job
            .check_runner
            .as_ref()
            .map(|runner| runner.platform.target())
            .or_else(|| crate::runs_on::target_for_runner(&job.runs_on))
            .ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
            })?;
        let setup = if job.check_runner.is_some() {
            mise.for_target(target)?
        } else {
            mise.clone()
        };
        cache_p08::ensure_tools_cache_v2(id, job, &setup, always, target, &ctx.checkout_uses)?;
        cache_p08::check_no_legacy_rust_cache(id, job)?;
        cache_p08::check_mbx_before_fetch(id, job)?;
        closure::check_internal_staged(id, job, ctx.preseed)?;
    }
    // Both cache families are validated globally before either gets a save.
    cache_p08::elect_cache_writers(&mut jobs)?;
    closure::check_plan_anchor(&jobs)?;
    preseed_closure::check_preseed_closure(&jobs, ctx.preseed)?;
    closure::insert_plan_closure(&mut jobs, ctx)?;
    closure::insert_request_closure(&mut jobs)?;
    closure::insert_task_closure(&mut jobs)?;
    closure::insert_final_closure(&mut jobs)?;
    final_steps::insert_final_fanin(&mut jobs, ctx)?;
    crate::dispatch_cache_boundary::suppress_unvalidated_cache_access(&mut jobs);
    validate_final_jobs(ir, &jobs)?;
    Ok(jobs)
}

/// Validate every finalized job after policy merging and internal expansion.
fn validate_final_jobs(ir: &WorkflowIr, jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    let mut finalized = ir.clone();
    finalized.jobs.clone_from(jobs);
    finalized.validate().map_err(RenderError::Contract)
}

/// Validate context/IR plus policy merge and support invariants.
fn merged_jobs(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<BTreeMap<String, Job>, RenderError> {
    ctx.validate()?;
    if ctx.preseed && policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Err(RenderError::PolicyRejected {
            policy: "consumer-v1".to_owned(),
            problem: "preseed_requires_velnor_policy".to_owned(),
        });
    }
    ir.validate().map_err(RenderError::Contract)?;
    workflow_policy::check_triggers(&ir.triggers)?;
    workflow_policy::check_concurrency(&ir.concurrency)?;
    workflow_policy::check_single_label(ir, &ctx.runs_on, &ctx.workflow_tasks)?;
    let mut jobs = ir.jobs.clone();
    match policy {
        WorkflowPolicy::ConsumerV1 => support::merge_consumer_verify(&mut jobs, support, ctx)?,
        WorkflowPolicy::VelnorRepositoryV1 => {
            support::merge_support_jobs(&mut jobs, support, ctx, "velnor-repository-v1")?;
        }
    }
    let workflow_task_ids =
        crate::verification_jobs::workflow_task_jobs::validate_workflow_task_jobs(
            &jobs,
            &ctx.workflow_tasks,
            &ctx.checkout_uses,
        )?;
    crate::verification_jobs::workflow_task_jobs::extend_required_needs(
        &mut jobs,
        &workflow_task_ids,
    )?;
    msrv::check_no_msrv(&jobs)?;
    support::check_candidate_invariants(&jobs)?;
    support::check_final_gate(&jobs)?;
    support::check_token_hygiene(&jobs)?;
    Ok(jobs)
}

/// Emit matrix strategy plus the quoted, marked workflow text.
fn render_merged(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<RenderedWorkflow, RenderError> {
    let matrix = matrix::task_matrix_of(jobs)?;
    let caps = matrix::crate_job_caps(jobs)?;
    let jobs = if matrix.is_some() || !caps.is_empty() {
        matrix::scrub_matrix_marker(jobs)
    } else {
        jobs.clone()
    };
    let mbx_jobs = crate::mbx_gc_policy::jobs_with_mbx_objects(&jobs);
    let shared = crate::lane_share::share_lanes(&jobs, ctx)?;
    let mut document = document::workflow_to_yaml(ir, &shared, ctx, &mbx_jobs)?;
    if let Some((source, max_parallel)) = &matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    matrix::attach_crate_job_caps(&mut document, &caps)?;
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    crate::workflow_size::check_workflow_size(WORKFLOW_PATH, &text)?;
    steps::scan_for_private_subcommands(&text)?;
    let shared_files = crate::render_cache_files::with_runtime_identity_files(
        shared.files,
        &jobs,
        &ctx.generator_version,
    )?;
    if crate::tool_seed::any_job_has_seed(&jobs)? {
        return Err(RenderError::InvalidWorkflow(
            "tool_seed_requires_tools_prelude".to_owned(),
        ));
    }
    Ok(RenderedWorkflow {
        yaml: text,
        shared: shared_files,
    })
}
