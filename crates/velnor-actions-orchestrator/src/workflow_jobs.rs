//! Workflow job constructors: plan, lint, and final gate.
//!
//! Crate jobs live in [`crate::crate_jobs`]: one ordered IR job per
//! crate, each needing the plan job; the final gate below needs them all.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, Step, StepKind};
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
/// through fail-closed `mise exec`: Rust plus the detected MBX driver for the
/// format/build steps, Nextest when any leg selects it, and the validators the
/// public `generate` runs inside `Check generated files`. Without it the freshness
/// step fails with `mise ... couldn't exec process` because implicit installation
/// is disabled there.
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
pub(crate) fn plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    use_mbx: bool,
    use_nextest: bool,
    fetch_roots: &[String],
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![checkout_action()?];
    let prepare = prepare_pinned_tools_step(catalog, plan_tools(use_mbx, use_nextest))?;
    steps.push(prepare);
    steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    let cached = cache_steps_for_plan(label, catalog, use_mbx, fetch_roots)?;
    steps.extend(cached.restore);
    steps.extend(fetch_steps_for_plan(catalog, fetch_roots)?);
    steps.extend(cached.save);
    steps.extend(acquire);
    steps.push(request_step(PLAN_OPERATION)?);
    steps.push(plan_step());
    Ok(Job {
        display_name: "Plan".to_owned(),
        runs_on: label.to_owned(),
        needs: Vec::new(),
        condition: None,
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
    Ok(Job {
        display_name: LINT_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs: Vec::new(),
        condition: None,
        steps: vec![
            checkout_action()?,
            Step {
                name: "Run actionlint".to_owned(),
                kind: StepKind::Shell {
                    run: argv,
                    env: BTreeMap::new(),
                },
            },
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
    steps.push(prepare_pinned_tools_step(catalog, vec![PinnedTool::Gh])?);
    steps.push(request_step(MERGE_OPERATION)?);
    steps.push(merge_step());
    Ok(Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        steps,
    })
}

/// Plan-job cache steps: restore before fetch, save after (writer only).
///
/// Lockless emits nothing. MBX repos restore/save the shared `actions/cache`
/// snapshot; Cargo-only repos emit one `rust-cache` writer step (its post
/// action saves; no separate save step).
struct PlanCache {
    /// Restore steps (before fetch).
    restore: Vec<Step>,
    /// Save steps (after fetch, writer only).
    save: Vec<Step>,
}

/// Cache steps for the plan job's trusted-writer role.
/// # Errors
///
/// Returns contract, actionlint, or render errors for bad labels or pins.
fn cache_steps_for_plan(
    label: &str,
    catalog: &ToolCatalog,
    use_mbx: bool,
    fetch_roots: &[String],
) -> Result<PlanCache, OrchestratorError> {
    if fetch_roots.is_empty() {
        return Ok(PlanCache {
            restore: Vec::new(),
            save: Vec::new(),
        });
    }
    let target = velnor_actions_contract::target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        }
    })?;
    let rust = catalog.version(PinnedTool::Rust);
    if use_mbx {
        let key = crate::source_cache::sources_cache_key(target, rust, fetch_roots)?;
        let prefix = crate::source_cache::sources_restore_prefix(&key);
        let restore = crate::source_cache::sources_restore_step(&key, &[prefix])?;
        let save = crate::source_cache::sources_save_step(&key)?;
        return Ok(PlanCache {
            restore: vec![restore],
            save: vec![save],
        });
    }
    let shared = format!(
        "{}-{target}-{rust}",
        crate::source_cache::RUST_CACHE_SHARED_PREFIX
    );
    let writer = crate::source_cache::rust_cache_step(&shared, true)?;
    Ok(PlanCache {
        restore: vec![writer],
        save: Vec::new(),
    })
}

/// Plan-job install set: driver tools, the `generate` validators, Nextest when used.
///
/// Validators join the driver set because `Check generated files` runs the public
/// `generate`, whose staged validation fail-closed-execs pinned actionlint, shellcheck,
/// and zizmor; installing only the driver toolchain leaves that step red. Order
/// follows `PinnedTool::ALL`.
fn plan_tools(use_mbx: bool, use_nextest: bool) -> Vec<PinnedTool> {
    let mut tools = vec![PinnedTool::Rust];
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools.extend([
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]);
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    tools
}

/// Typed `Prepare pinned tools` step for one exact tool set.
///
/// Homes use the runner-temp expression form: shell `$VAR` never expands
/// in the `env:` position that carries these paths.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
fn prepare_pinned_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
) -> Result<Step, OrchestratorError> {
    let homes = ToolHomes::runner_temp();
    let prepare =
        PreparePinnedTools::new(tools, homes).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = strings_of_env(&prepare.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(Step {
        name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
        kind: StepKind::Shell { run, env },
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Internal operation of one step, if any.
    fn operation_of(step: &Step) -> Option<&str> {
        match &step.kind {
            StepKind::Internal { operation } => Some(operation),
            StepKind::Action { .. } | StepKind::Shell { .. } => None,
        }
    }

    /// Assert Write request precedes `target` with the expected operations.
    fn assert_request_before(job: &Job, target: &str, request: &str, operation: &str) {
        let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
        let write_at = names.iter().position(|name| *name == "Write request");
        let target_at = names.iter().position(|name| *name == target);
        assert!(
            write_at.is_some_and(|write| Some(write) < target_at),
            "request must precede {target}: {names:?}"
        );
        assert_eq!(
            operation_of(&job.steps[write_at.expect("write request step")]),
            Some(request),
            "request must target {target}"
        );
        assert_eq!(
            operation_of(&job.steps[target_at.expect("target step")]),
            Some(operation)
        );
    }

    #[test]
    fn plan_job_writes_request_before_plan() {
        let catalog = ToolCatalog::pinned();
        for acquire in [None, Some(checkout_action().expect("checkout step"))] {
            let job =
                plan_job("ubuntu-26.04", acquire, &catalog, false, false, &[]).expect("plan job");
            assert_request_before(&job, "Plan", "write-request-v1:plan-v1", PLAN_OPERATION);
        }
    }

    #[test]
    fn plan_job_prepares_pinned_tools_before_generate_consumers() {
        let catalog = ToolCatalog::pinned();
        for (use_mbx, use_nextest) in [(false, false), (false, true), (true, true)] {
            let job = plan_job("ubuntu-26.04", None, &catalog, use_mbx, use_nextest, &[])
                .expect("plan job");
            let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
            let prepare_at = names
                .iter()
                .position(|name| *name == PREPARE_PINNED_TOOLS_STEP);
            assert_eq!(
                prepare_at,
                Some(1),
                "prepare sits after checkout: {names:?}"
            );
            let write_at = names.iter().position(|name| *name == "Write request");
            let plan_at = names.iter().position(|name| *name == "Plan");
            assert!(
                prepare_at
                    .is_some_and(|prepare| Some(prepare) < write_at && Some(prepare) < plan_at),
                "prepare must precede request and plan: {names:?}"
            );
            let StepKind::Shell { run, env } = &job.steps[prepare_at.expect("prepare step")].kind
            else {
                panic!("prepare must be a shell step: {names:?}");
            };
            assert_eq!(run[0], "mise");
            let install_at = run.iter().position(|arg| arg == "install");
            let mut specs = vec![
                catalog.tool_spec(PinnedTool::Rust),
                catalog.tool_spec(PinnedTool::Actionlint),
                catalog.tool_spec(PinnedTool::Shellcheck),
                catalog.tool_spec(PinnedTool::Zizmor),
            ];
            if use_mbx {
                specs.insert(1, catalog.tool_spec(PinnedTool::MrBoxington));
            }
            if use_nextest {
                specs.push(catalog.tool_spec(PinnedTool::Nextest));
            }
            assert_eq!(
                install_at.map(|at| &run[at + 1..]),
                Some(specs.as_slice()),
                "install specs: {run:?}"
            );
            let keys = [
                "MISE_RUSTUP_HOME",
                "MISE_CARGO_HOME",
                "RUSTUP_TOOLCHAIN",
                "MISE_LOCKFILE",
            ];
            for key in keys {
                assert!(env.contains_key(key), "env misses {key}: {env:?}");
            }
        }
    }

    #[test]
    fn final_job_writes_request_before_merge() {
        let catalog = ToolCatalog::pinned();
        for acquire in [None, Some(checkout_action().expect("checkout step"))] {
            let job = final_job("ubuntu-26.04", &["rust-demo".to_owned()], acquire, &catalog)
                .expect("final job");
            assert_request_before(
                &job,
                "Merge reports",
                "write-request-v1:merge-v1",
                MERGE_OPERATION,
            );
        }
    }

    #[test]
    fn final_job_needs_plan_crates_and_lint() {
        let catalog = ToolCatalog::pinned();
        let job = final_job(
            "ubuntu-26.04",
            &["rust-demo".to_owned(), "rust-nested".to_owned()],
            None,
            &catalog,
        )
        .expect("final job");
        assert_eq!(
            job.needs,
            [
                PLAN_JOB_ID.to_owned(),
                "rust-demo".to_owned(),
                "rust-nested".to_owned(),
                LINT_JOB_ID.to_owned(),
            ]
        );
        let job = final_job("ubuntu-26.04", &[], None, &catalog).expect("final job");
        assert_eq!(job.needs, [PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()]);
    }
}
