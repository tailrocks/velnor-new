//! Workflow job constructors: plan, lint, and final gate.
//!
//! Crate jobs live in [`crate::crate_jobs`]: one ordered IR job per
//! crate, each needing the plan job; the final gate below needs them all.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_contract_workflow::{Job, JobTimeout, Permissions, Step, StepRole};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PinnedToolExec, PreparePinnedTools, ToolCatalog,
    ToolHomes,
};
use velnor_actions_workflow_renderer::render::{FINAL_CONDITION, FINAL_DISPLAY_NAME, PLAN_JOB_ID};
use velnor_actions_workflow_renderer::steps::{
    MERGE_OPERATION, PLAN_OPERATION, merge_step, plan_step, write_request_step,
};

use crate::OrchestratorError;
use crate::source_prep::fetch_steps_for_plan;
use crate::utf8::{strings_of, strings_of_env};

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "actionlint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Actionlint";

/// Planner job: checkout, pinned-tool install, optional Acquire, request, plan.
///
/// `Prepare pinned tools` installs the exact catalog tools the later steps consume
/// through fail-closed `mise exec`, per role: Rust plus the detected MBX driver
/// for the format/build steps, Nextest when any leg selects it, Opentofu when any
/// tofu work exists, and the validators the public `generate` runs inside `Check
/// generated files`. Without it the freshness step fails with `mise ...
/// couldn't exec process` because implicit installation is disabled there.
/// Pure-tofu plans install opentofu plus the validators with no Rust setup
/// (no components, fetch, or owned-homes triple); mixed plans the union.
///
/// No `Verify toolchain` step: task-execution-contract §2 scopes it to
/// task jobs; the plan sequence is workflow-contract §3 steps 1-8.
///
/// The write-request step materializes the event request file the plan
/// step's private gate requires; without it the helper falls through to
/// CLI usage and the job (plus its `Publish plan` upload) fails.
/// `Fetch Cargo sources` runs `cargo fetch --locked` per lockful
/// workspace ahead of every locked/offline consumer (Gate 1).
/// # Errors
///
/// Returns a contract error when a typed step request is rejected.
#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one call site threads job scope plus role selection"
)]
pub(crate) fn plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    fetch_roots: &[String],
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![checkout_history_action()?];
    let prepare = prepare_pinned_tools_step(
        catalog,
        plan_tools(use_rust, use_nextest, use_opentofu),
        use_rust,
    )?;
    steps.push(prepare);
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    let cached =
        crate::workflow_jobs_cache::cache_steps_for_plan(label, catalog, use_mbx, fetch_roots)?;
    steps.extend(cached.restore);
    steps.extend(fetch_steps_for_plan(catalog, fetch_roots)?);
    steps.extend(cached.save);
    steps.extend(acquire);
    steps.push(request_step(PLAN_OPERATION)?);
    steps.push(plan_step());
    Ok(Job {
        display_name: "Plan".to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
    })
}

/// Always-on lint job: checkout plus pinned actionlint over the tree.
pub(crate) fn lint_job(label: &str, catalog: &ToolCatalog) -> Result<Job, OrchestratorError> {
    let program = OsString::from("actionlint");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Actionlint, PinnedTool::Shellcheck],
        &program,
        vec![OsString::from("-color")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let argv = strings_of(exec.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let mut lint = velnor_actions_workflow_renderer::ambient_shell_step(
        "Run actionlint",
        argv,
        BTreeMap::new(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    lint.role = Some(StepRole::Actionlint);
    Ok(Job {
        display_name: LINT_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checkout_action()?, lint],
    })
}

/// Final gate with the exact required-check name and `always()` condition.
///
/// Needs decision (workflow-contract §4 "depends on the base and enabled
/// policy jobs" + "`Required` depends on the plan, every crate
/// job ... and the candidate report when candidate mode is enabled"):
/// plan + every crate job + lint always, plus alint/policy/candidate when
/// those policy jobs exist. IR validation requires `needs` to name IR jobs
/// only, so this builds the IR subset and the renderer appends the merged
/// support IDs post-merge (see `support.rs`); the release job never gates.
/// `Prepare pinned tools` installs only `gh`: the fetch step downloads
/// each expected matrix artifact by exact name through it. The
/// write-request step assembles the merge request from the downloaded
/// plan and matrix-report artifacts the merge step consumes.
/// # Errors
///
/// Returns a contract error when a typed step request is rejected.
pub(crate) fn final_job(
    label: &str,
    crate_job_ids: &[String],
    acquire: Option<Step>,
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    needs.extend(crate_job_ids.iter().cloned());
    needs.push(LINT_JOB_ID.to_owned());
    let mut steps = Vec::new();
    steps.extend(acquire);
    steps.push(prepare_pinned_tools_step(
        catalog,
        vec![PinnedTool::Gh],
        true,
    )?);
    steps.push(request_step(MERGE_OPERATION)?);
    steps.push(merge_step());
    Ok(Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        permissions: Some(read_actions_permissions()),
        environment: None,
        steps,
    })
}

/// Job-level permissions for the two authenticated Actions-artifact readers.
pub(crate) fn read_actions_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Read,
    }
}

/// Plan-job install set per role: drivers, the `generate` validators, Nextest when used.
///
/// Validators join the driver set because `Check generated files` runs the public
/// `generate`, whose staged validation fail-closed-execs pinned actionlint, shellcheck,
/// and zizmor; installing only the driver toolchain leaves that step red. Order
/// follows `PinnedTool::ALL`. Pure-tofu plans carry opentofu plus the validators
/// with no Rust; mixed plans carry the union.
fn plan_tools(use_rust: bool, use_nextest: bool, use_opentofu: bool) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend([
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]);
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Typed `Prepare pinned tools` step for one exact tool set.
///
/// Homes use the runner-temp expression form: shell `$VAR` never expands
/// in the `env:` position that carries these paths. Pure-tofu roles
/// (`use_rust` false) carry no owned-homes triple; every other role
/// keeps it.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
fn prepare_pinned_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
    use_rust: bool,
) -> Result<Step, OrchestratorError> {
    let homes = ToolHomes::runner_temp();
    let prepare =
        PreparePinnedTools::new(tools, homes).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if use_rust {
        strings_of_env(&prepare.env(catalog))
    } else {
        strings_of_env(&prepare.env_without_homes())
    }
    .map_err(|problem| OrchestratorError::Contract { problem })?;
    let mut step =
        velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
            .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

/// Typed write-request step for one internal target, mapped to contract errors.
fn request_step(target: &str) -> Result<Step, OrchestratorError> {
    write_request_step(target).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

/// Pinned checkout action without persisted credentials.
fn checkout_action() -> Result<Step, OrchestratorError> {
    crate::workflow::wire_w1::checkout_step()
}

/// Pinned checkout with full history for the plan job's git archaeology.
fn checkout_history_action() -> Result<Step, OrchestratorError> {
    crate::workflow::wire_w1::checkout_step_full()
}

#[cfg(test)]
mod tests;
