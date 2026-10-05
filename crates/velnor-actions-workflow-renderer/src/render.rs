//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates: exact triggers, concurrency, single runner label,
//! consumer support rejection, and candidate-never-plans invariants.
//! The strict entrypoint additionally mandates Mise setup, staged
//! helpers, and the plan anchor; the legacy entrypoint preserves the
//! previous contract for in-flight callers.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, ValidatorKind, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};

use crate::{
    RenderError, cache_p08, closure, commands, document, final_steps, guard, marker, matrix, msrv,
    preseed_closure, steps, support, workflow_policy, yaml::render_yaml,
};

pub use crate::render_constants::*;

#[path = "render_action_pins.rs"]
mod action_pins_impl;
pub use action_pins_impl::action_pins;

pub use crate::matrix::{
    COVERED_TASKS_OUTPUT, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
    MatrixSource, PLAN_ID_OUTPUT, PLAN_STEP_ID, RUN_KEY_OUTPUT,
};
pub use crate::setup::MiseSetup;

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
    /// Sorted isolated verification jobs with per-runner Mise pins.
    pub verification_tasks: Vec<crate::VerificationTaskPolicy>,
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
pub use velnor_actions_contract::{AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET};

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
        if ctx
            .verification_tasks
            .iter()
            .any(|task| task.owns_job_id(id))
        {
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
    // Validate both cache families before either receives a save step.
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

/// Revalidate each complete job after policy merge and all internal expansion.
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
    workflow_policy::check_single_label(ir, &ctx.runs_on, &ctx.verification_tasks)?;
    let mut jobs = ir.jobs.clone();
    match policy {
        WorkflowPolicy::ConsumerV1 => support::reject_consumer_support(&jobs, support)?,
        WorkflowPolicy::VelnorRepositoryV1 => {
            support::merge_support_jobs(&mut jobs, support, ctx)?;
        }
    }
    let verification_ids = crate::verification_jobs::validate_verification_jobs(
        &jobs,
        &ctx.verification_tasks,
        &ctx.checkout_uses,
    )?;
    crate::verification_jobs::extend_required_needs(&mut jobs, &verification_ids)?;
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
    let identity_lanes: BTreeSet<String> = jobs
        .values()
        .filter(|job| {
            job.steps.iter().any(|step| {
                step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity)
            })
        })
        .map(|job| job.runs_on.clone())
        .collect();
    let shared = crate::lane_share::share_lanes(&jobs, ctx)?;
    let mut document = document::workflow_to_yaml(ir, &shared, ctx, &mbx_jobs)?;
    if let Some((source, max_parallel)) = &matrix {
        matrix::attach_task_matrix(&mut document, source, *max_parallel)?;
    } else {
        matrix::attach_plan_outputs(&mut document)?;
    }
    matrix::attach_crate_job_caps(&mut document, &caps)?;
    let document = crate::yaml::quote_run_values_in_yaml(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    crate::workflow_size::check_workflow_size(WORKFLOW_PATH, &text)?;
    steps::scan_for_private_subcommands(&text)?;
    let mut files = shared.files;
    for runs_on in &identity_lanes {
        files.push(cache_p08::runtime_identity_action_file(
            runs_on,
            &ctx.generator_version,
        )?);
    }
    if !identity_lanes.is_empty() {
        files.push(cache_p08::runtime_identity_script_file(
            &ctx.generator_version,
        )?);
    }
    if crate::tool_seed::any_job_has_seed(&jobs) {
        files.push(crate::tool_seed::action_file(&ctx.generator_version)?);
    }
    Ok(RenderedWorkflow {
        yaml: text,
        shared: files,
    })
}
