//! W1 emission wiring: orchestrator halves of cross-crate TODO rows.
//!
//! Self-declared from `workflow.rs` (`#[path]`, no `lib.rs` edit) so the
//! integrator only registers the companion test file. Every helper here
//! answers one anchored EMIT entry: checkout provenance, step-syntax
//! vetting, task-job prelude assembly, plan format, MBX gating, the
//! Gate-6 task-cache gate, and the V1 actionlint variable set.

use std::collections::BTreeMap;

use velnor_actions_actionlint::{
    ActionlintCapabilities, PinnedActionRef, StepSyntax,
    actions::{
        CACHE_ACTION_SHA, CACHE_ACTION_VERSION, MR_BOXINGTON_ACTION_SHA,
        MR_BOXINGTON_ACTION_VERSION,
    },
    checkout_inputs_schema, validate_action_inputs,
};
use velnor_actions_contract::{Job, Step, StepKind};
use velnor_actions_mise::{Gate6Fixture, TaskCacheMode, ToolCatalog, ToolHomes};
use velnor_actions_rust::{TaskKind, derive_workspace_fmt};
use velnor_actions_workflow_renderer::plan_format;
use velnor_actions_workflow_renderer::render::{PLAN_JOB_ID, TASK_JOB_ID};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, TASK_ARTIFACTS_DIR, cache_action_step, check_mbx_gating, mbx_step_for_driver,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::matrix_step::{matrix_task_step, prepare_task_tools_step, task_toolchain_env};
use crate::source_prep::fetch_steps;
use crate::utf8::strings_of_env;

/// Declared repository configuration variable names (GEN-2.14).
///
/// V1 emits no `vars:` references, so the exact declared set is empty;
/// the renderer-side set threads through this single constructor when a
/// future workflow declares variables.
#[must_use]
pub(crate) fn declared_config_variables() -> Vec<String> {
    Vec::new()
}

/// Pinned checkout action without persisted credentials (WF-3.50).
///
/// The `uses:` value comes from the canonical actionlint ref and the
/// inputs validate against the checkout schema before construction, so
/// generation aborts pre-write on unknown/missing/empty inputs.
///
/// # Errors
///
/// Returns a contract error when the fixed inputs fail schema validation.
pub(crate) fn checkout_step() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    validate_action_inputs(&checkout_inputs_schema(), &with).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        name: "Checkout".to_owned(),
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
        },
    })
}

/// Reject workflow syntax the pinned actionlint cannot parse (PAR-6.1).
///
/// Called for `JobMatrix` on every generation; native step parallelism
/// stays unqualified, so any future native-key emission fails here.
///
/// # Errors
///
/// Returns an actionlint error for unqualified syntax.
pub(crate) fn vet_step_syntax(syntax: StepSyntax) -> Result<(), OrchestratorError> {
    ActionlintCapabilities::for_pinned()
        .check_step_syntax(syntax)
        .map_err(OrchestratorError::from)
}

/// Matrix consumer job: checkout, pinned tools, sources, MBX, template.
///
/// Order follows task-execution-contract §2: prepare, components, per-root
/// `Fetch Cargo sources` (sharing the `Run task` toolchain env so cold
/// locked/offline payloads resolve), MBX objects restore (MBX legs only),
/// then the fixed matrix-entry template. Gate-6 cache steps stay absent
/// until a qualification fixture enables them.
///
/// # Errors
///
/// Returns a contract error when a typed step request is rejected.
pub(crate) fn build_task_job(
    label: &str,
    max_parallel_jobs: u32,
    catalog: &ToolCatalog,
    use_mbx: bool,
    use_nextest: bool,
    fetch_roots: &[String],
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![checkout_step()?];
    steps.push(prepare_task_tools_step(catalog, use_mbx, use_nextest)?);
    steps.push(super::prepare_rust_components_step(catalog)?);
    steps.extend(fetch_steps(
        catalog,
        fetch_roots,
        &task_toolchain_env(catalog),
    )?);
    steps.extend(mbx_task_step(use_mbx)?);
    steps.extend(maybe_task_cache_steps(None, TaskCacheMode::Off, "")?);
    steps.push(matrix_task_step(max_parallel_jobs, catalog));
    Ok(Job {
        display_name: "Velnor Task".to_owned(),
        runs_on: label.to_owned(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps,
    })
}

/// MBX objects restore for MBX legs only (WF-3.52).
///
/// The `uses:` value comes from the compiled action-registry pin; Cargo
/// legs carry neither the action nor the tool.
///
/// # Errors
///
/// Returns actionlint/render errors for a rejected pin or step shape.
fn mbx_task_step(use_mbx: bool) -> Result<Option<Step>, OrchestratorError> {
    if !use_mbx {
        return Ok(None);
    }
    let uses = PinnedActionRef::new(
        "jdx/mr-boxington-action",
        None,
        MR_BOXINGTON_ACTION_SHA,
        MR_BOXINGTON_ACTION_VERSION,
    )?
    .uses_value();
    Ok(mbx_step_for_driver(&uses, CompileDriver::Mbx)?)
}

/// Task-layer restore/save pair, gated on Gate-6 qualification.
///
/// Emits the restore step before and the save step after the matrix
/// payload only with a qualification fixture and a live cache mode;
/// release legs (`Off`) and unqualified generation emit neither.
///
/// # Errors
///
/// Returns actionlint/render errors for rejected pins or step shapes.
pub(crate) fn maybe_task_cache_steps(
    fixture: Option<&Gate6Fixture>,
    mode: TaskCacheMode,
    key: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    if fixture.is_none() || mode == TaskCacheMode::Off {
        return Ok(Vec::new());
    }
    let restore_uses = PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value();
    let save_uses = PinnedActionRef::new(
        "actions/cache",
        Some("save"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value();
    let paths = vec![TASK_ARTIFACTS_DIR.to_owned()];
    Ok(vec![
        cache_action_step(true, &restore_uses, "task", key, &[], &paths)?,
        cache_action_step(false, &save_uses, "task", key, &[], &paths)?,
    ])
}

/// Insert the plan-job `Format` step for selected Rust work.
///
/// Matrix fmt groups exist only with explicit `rustfmt` config
/// (task-execution-contract: per-package formatting needs explicit
/// configuration), but the plan-job `Format` step runs once per selected
/// stack configuration by default (workflow-contract §3 plan step 5).
/// With an explicit fmt group the argv comes from it; otherwise it is
/// synthesized from the first planned workspace profile (MBX route when
/// the profile selects it). The step carries the full `exec`
/// verification env so it runs the prepared toolchain. Without selected
/// Rust work there is nothing to format.
///
/// # Errors
///
/// Returns contract/render errors for rejected vectors or step shapes.
pub(crate) fn ensure_plan_format_step(
    jobs: &mut BTreeMap<String, Job>,
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    let env = format_step_env(catalog)?;
    if let Some(fmt) = discovery
        .task_groups
        .iter()
        .find(|group| matches!(group.kind, TaskKind::Fmt))
    {
        let argv = crate::vectors::task_argv(fmt, catalog)?;
        return plan_format::ensure_plan_format(jobs, argv, env).map_err(OrchestratorError::from);
    }
    let (Some(workspace), Some(template)) =
        (discovery.workspaces.first(), discovery.task_groups.first())
    else {
        return Ok(());
    };
    let manifest = crate::discover::workspace_manifest(&workspace.record.workspace_root);
    let fmt = derive_workspace_fmt(
        &manifest,
        &workspace.profile,
        &template.configuration,
        &template.target,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let argv = crate::vectors::task_argv(&fmt, catalog)?;
    plan_format::ensure_plan_format(jobs, argv, env).map_err(OrchestratorError::from)
}

/// Full `exec` verification env routing Format at the prepared toolchain.
///
/// Without the owned homes the step resolves whatever ambient toolchain
/// the runner offers (a minimal image toolchain has no `rustfmt`); with
/// them it runs the `Prepare pinned tools` toolchain, and a missing tool
/// fails as a preparation error instead of installing.
fn format_step_env(catalog: &ToolCatalog) -> Result<BTreeMap<String, String>, OrchestratorError> {
    strings_of_env(&ToolHomes::runner_temp().exec_env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })
}

/// Gate MBX presence in the task job against its driver selection.
///
/// Cargo legs must be MBX-free; MBX legs carry exactly one
/// objects-mode step. Jobs without a task job are vacuous.
///
/// # Errors
///
/// Returns a render error when MBX presence mismatches the driver.
pub(crate) fn check_task_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    task_present: bool,
    use_mbx: bool,
) -> Result<(), OrchestratorError> {
    if !task_present {
        return Ok(());
    }
    let driver = if use_mbx {
        CompileDriver::Mbx
    } else {
        CompileDriver::Cargo
    };
    check_mbx_gating(jobs, &BTreeMap::from([(TASK_JOB_ID.to_owned(), driver)]))
        .map_err(OrchestratorError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_pins_canonical_ref_and_validates_inputs() {
        let step = checkout_step().expect("checkout step");
        let StepKind::Action { uses, with } = &step.kind else {
            panic!("checkout must be an action step");
        };
        assert_eq!(uses, &PinnedActionRef::checkout().uses_value());
        assert_eq!(uses, crate::workflow::CHECKOUT_USES);
        assert!(validate_action_inputs(&checkout_inputs_schema(), with).is_ok());
        for (name, inputs) in [
            (
                "unknown",
                BTreeMap::from([("bogus".to_owned(), "x".to_owned())]),
            ),
            ("missing", BTreeMap::new()),
            (
                "empty",
                BTreeMap::from([("persist-credentials".to_owned(), String::new())]),
            ),
        ] {
            assert!(
                validate_action_inputs(&checkout_inputs_schema(), &inputs).is_err(),
                "{name} inputs must fail"
            );
        }
    }

    #[test]
    fn syntax_gate_allows_matrix_and_rejects_native() {
        assert!(vet_step_syntax(StepSyntax::JobMatrix).is_ok());
        let err = vet_step_syntax(StepSyntax::NativeParallelism).expect_err("native gated");
        assert!(err.to_string().contains("native_parallelism"), "{err}");
    }

    #[test]
    fn task_cache_steps_need_fixture_and_live_mode() {
        assert!(maybe_task_cache_steps(None, TaskCacheMode::ReadWrite, "k").is_ok());
        assert!(
            maybe_task_cache_steps(None, TaskCacheMode::ReadWrite, "k")
                .expect("v")
                .is_empty()
        );
        let fixture = Gate6Fixture::new("gate6/w1").expect("fixture");
        assert!(
            maybe_task_cache_steps(Some(&fixture), TaskCacheMode::Off, "k")
                .expect("off")
                .is_empty()
        );
        let steps =
            maybe_task_cache_steps(Some(&fixture), TaskCacheMode::ReadOnly, "velnor-v1-task-k")
                .expect("gated steps");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "Restore cache");
        assert_eq!(steps[1].name, "Save cache");
    }

    #[test]
    fn mbx_and_driver_tools_follow_selection() {
        use velnor_actions_mise::PinnedTool;

        use crate::matrix_step::task_driver_tools;
        assert_eq!(task_driver_tools(false), vec![PinnedTool::Rust]);
        assert_eq!(
            task_driver_tools(true),
            vec![PinnedTool::Rust, PinnedTool::MrBoxington]
        );
        assert!(mbx_task_step(false).expect("cargo none").is_none());
        let step = mbx_task_step(true).expect("mbx step").expect("mbx some");
        let StepKind::Action { uses, with } = &step.kind else {
            panic!("mbx must be an action step");
        };
        assert!(uses.starts_with("jdx/mr-boxington-action@"), "{uses}");
        assert_eq!(
            with.get("github-cache-mode").map(String::as_str),
            Some("objects")
        );
        assert!(PinnedActionRef::parse_uses(uses, MR_BOXINGTON_ACTION_VERSION).is_ok());
        assert!(declared_config_variables().is_empty());
    }

    #[test]
    fn task_prepare_steps_carry_nextest_and_components() {
        use velnor_actions_mise::{
            PREPARE_PINNED_TOOLS_STEP, PREPARE_RUST_COMPONENTS_STEP, PinnedTool,
        };
        let catalog = ToolCatalog::pinned();
        for use_nextest in [false, true] {
            let job = build_task_job("ubuntu-26.04", 2, &catalog, false, use_nextest, &[])
                .expect("task job");
            assert_eq!(job.steps[1].name, PREPARE_PINNED_TOOLS_STEP);
            assert_eq!(job.steps[2].name, PREPARE_RUST_COMPONENTS_STEP);
            let StepKind::Shell { run, .. } = &job.steps[1].kind else {
                panic!("prepare must be a shell step");
            };
            let nextest = catalog.tool_spec(PinnedTool::Nextest);
            assert_eq!(run.contains(&nextest), use_nextest);
        }
    }
}
