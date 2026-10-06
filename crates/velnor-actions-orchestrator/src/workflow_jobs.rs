//! Workflow job constructors: plan, lint, and final gate.
//!
//! Crate jobs live in [`crate::crate_jobs`]: one ordered IR job per
//! crate, each needing the plan job; the final gate below needs them all.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, JobTimeout, Step};
use velnor_actions_mise::catalog::rust_prepare::RustPrepareDomain;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, PreparePinnedTools, ToolCatalog, ToolHomes};
use velnor_actions_workflow_renderer::render::{FINAL_CONDITION, FINAL_DISPLAY_NAME, PLAN_JOB_ID};
use velnor_actions_workflow_renderer::steps::{
    MERGE_OPERATION, PLAN_OPERATION, merge_step, plan_step, write_request_step,
};

#[path = "rust_tools_prepare.rs"]
pub(crate) mod rust_tools_prepare;

use crate::OrchestratorError;
use crate::source_prep::fetch_steps_for_plan;
use crate::utf8::{strings_of, strings_of_env};

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "actionlint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Actionlint";

/// Planner job: verified consumer analysis precedes Cargo preparation.
///
/// Consumers acquire the helper and prepare only Gh plus generation validators
/// before attempting authenticated Cargo-free planning. A typed cache miss
/// enables full tool, component, and source preparation. Repository dogfood
/// follows its fresh source-build path without this boundary.
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
    let early = acquire.is_some();
    if let Some(acquire) = acquire {
        steps.extend(consumer_planning_steps(acquire, catalog)?);
    }
    let expensive_at = steps.len();
    let mut tools = plan_tools(use_rust, use_mbx, use_nextest, use_opentofu);
    if early {
        tools.retain(|tool| {
            !matches!(
                tool,
                PinnedTool::Actionlint | PinnedTool::Shellcheck | PinnedTool::Zizmor
            )
        });
    }
    if !tools.is_empty() {
        steps.push(prepare_pinned_tools_step_for_runner(
            catalog, tools, use_rust, label,
        )?);
    }
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    steps.extend(fetch_steps_for_plan(catalog, fetch_roots)?);
    if !early {
        steps.push(request_step(PLAN_OPERATION)?);
    }
    if early {
        for step in &mut steps[expensive_at..] {
            velnor_actions_workflow_renderer::early_plan::require_cargo(step);
        }
    }
    steps.push(plan_step());
    steps.push(velnor_actions_workflow_renderer::analysis_publication::upload_step()?);
    Ok(Job {
        cache_mode: None,
        display_name: "Plan".to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps,
    })
}

/// Only the verified helper and pinned planning tools precede the Cargo boundary.
fn consumer_planning_steps(
    acquire: Step,
    catalog: &ToolCatalog,
) -> Result<Vec<Step>, OrchestratorError> {
    let prepare = prepare_planning_tools_step(
        catalog,
        vec![
            PinnedTool::Gh,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ],
    )?;
    Ok(vec![
        acquire,
        prepare,
        request_step(PLAN_OPERATION)?,
        velnor_actions_workflow_renderer::early_plan::early_plan_step()?,
    ])
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
    let argv = strings_of(exec.argv(catalog)?)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(Job {
        cache_mode: None,
        display_name: LINT_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![
            checkout_action()?,
            velnor_actions_workflow_renderer::ambient_shell_step(
                "Run actionlint",
                argv,
                BTreeMap::new(),
            )
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?,
        ],
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
    steps.push(prepare_pinned_tools_step_for_runner(
        catalog,
        vec![PinnedTool::Gh],
        false,
        label,
    )?);
    steps.push(request_step(MERGE_OPERATION)?);
    steps.push(merge_step());
    Ok(Job {
        cache_mode: None,
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::REQUIRED,
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps,
    })
}

/// Plan-job install set per role: drivers, the `generate` validators, Nextest when used.
///
/// Validators join the driver set because `Check generated files` runs the public
/// `generate`, whose staged validation fail-closed-execs pinned actionlint, shellcheck,
/// and zizmor; installing only the driver toolchain leaves that step red. Order
/// follows `PinnedTool::ALL`. Pure-tofu plans carry opentofu plus the validators
/// with no Rust; mixed plans carry the union.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "four independent install flags mirror the role selection"
)]
fn plan_tools(
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools.extend([
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]);
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Select installations using the actual workflow runner's qualified host.
pub(crate) fn prepare_pinned_tools_step_for_runner(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
    use_rust: bool,
    label: &str,
) -> Result<Step, OrchestratorError> {
    let host = velnor_actions_contract::tool_target_for_runner_label(label)
        .and_then(velnor_actions_mise::catalog::qualification::DistributionHost::for_target)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("prepare_pinned_tools_host:{label}"),
        })?;
    prepare_tools_step_in_domain(
        catalog,
        tools,
        use_rust,
        RustPrepareDomain::Tools,
        Some(host),
    )
}

/// Minimal helper installation owns its dedicated planning bootstrap domain.
pub(crate) fn prepare_planning_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
) -> Result<Step, OrchestratorError> {
    let mut step = prepare_tools_step_in_domain(
        catalog,
        tools,
        false,
        RustPrepareDomain::PlanningBootstrap,
        None,
    )?;
    step.name = "Prepare planning tools".to_owned();
    Ok(step)
}

fn prepare_tools_step_in_domain(
    catalog: &ToolCatalog,
    mut tools: Vec<PinnedTool>,
    use_rust: bool,
    domain: RustPrepareDomain,
    host: Option<velnor_actions_mise::catalog::qualification::DistributionHost>,
) -> Result<Step, OrchestratorError> {
    let install_rust = tools
        .iter()
        .any(|tool| matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop));
    for tool in &mut tools {
        if matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop) {
            *tool = catalog.compiler_tool();
        }
    }
    let homes = ToolHomes::runner_temp();
    let prepare =
        PreparePinnedTools::new(tools, homes).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let argv = match host {
        Some(host) => prepare.argv_for_host(catalog, host)?,
        None => prepare.argv(catalog)?,
    };
    let run = strings_of(argv).map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if use_rust {
        strings_of_env(&prepare.env(catalog))
    } else {
        strings_of_env(&PreparePinnedTools::env_for_domain(
            domain.bootstrap_domain(),
        ))
    }
    .map_err(|problem| OrchestratorError::Contract { problem })?;
    rust_tools_prepare::prepare_step_in_domain(run, env, install_rust, catalog, domain)
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
#[path = "workflow_jobs_tests.rs"]
mod workflow_jobs_tests;

#[cfg(test)]
#[path = "workflow_full_home_tests.rs"]
mod full_home_tests;
