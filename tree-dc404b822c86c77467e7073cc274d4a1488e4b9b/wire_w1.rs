//! W1 emission wiring: orchestrator halves of cross-crate TODO rows.
//!
//! Self-declared from `workflow.rs` (`#[path]`, no `lib.rs` edit) so the
//! integrator only registers the companion test file. Every helper here
//! answers one anchored EMIT entry: checkout provenance, step-syntax
//! vetting, the plan-job workspace Format step, MBX gating, the Gate-6
//! task-cache gate, and the V1 actionlint variable set.

use std::collections::BTreeMap;

use velnor_actions_actionlint::{
    ActionlintCapabilities, PinnedActionRef, StepSyntax,
    actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION},
    checkout_inputs_schema, validate_action_inputs,
};
use velnor_actions_contract::{Job, Step, StepKind};
use velnor_actions_mise::{Gate6Fixture, TaskCacheMode, ToolCatalog, ToolHomes};
use velnor_actions_rust::TaskKind;
use velnor_actions_workflow_renderer::plan_format;
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, TASK_ARTIFACTS_DIR, cache_action_step, check_mbx_gating,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
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
        condition: None,
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
        },
    })
}

/// Checkout with full history for git-archaeology jobs (plan only).
///
/// Plan verifies the checkout against the PR head through the merge
/// commit's second parent and diffs `base...head` for affected work;
/// both need history the default depth-1 checkout never fetches, so a
/// shallow plan checkout fails closed with `checkout_head_mismatch` on
/// every pull-request run (CI run 36749240499).
///
/// # Errors
///
/// Returns a contract error when the fixed inputs fail schema validation.
pub(crate) fn checkout_step_full() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("fetch-depth".to_owned(), "0".to_owned()),
    ]);
    validate_action_inputs(&checkout_inputs_schema(), &with).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        name: "Checkout".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
        },
    })
}

/// Reject workflow syntax the pinned actionlint cannot parse (PAR-6.1).
///
/// Native step parallelism stays unqualified, so any future native-key
/// emission fails here.
///
/// # Errors
///
/// Returns an actionlint error for unqualified syntax.
pub(crate) fn vet_step_syntax(syntax: StepSyntax) -> Result<(), OrchestratorError> {
    ActionlintCapabilities::for_pinned()
        .check_step_syntax(syntax)
        .map_err(OrchestratorError::from)
}

/// Task-layer restore/save pair, gated on Gate-6 qualification.
///
/// Emits the restore step before and the save step after the
/// obligation payload only with a qualification fixture and a live
/// cache mode; release legs (`Off`) and unqualified generation emit
/// neither.
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

/// Plan-job `Format` step for the workspace formatting scope only.
///
/// Root cause (P05-5): the plan job duplicated the first crate's
/// formatting and synthesized an overlapping whole-workspace format,
/// so one scope had three owners. Per-package formatting lives in
/// crate jobs; this returns a step only for a package-less workspace
/// `Fmt` obligation (explicit root `rustfmt` config), the one distinct
/// scope the plan job owns. Derivation (`derive_for_config`) suppresses
/// the workspace group whenever per-package `Fmt` groups exist for the
/// same config, so this step never re-checks crate-owned files (R28).
/// No synthesis, no fallback.
///
/// # Errors
///
/// Returns contract/render errors for rejected vectors or step shapes.
pub(crate) fn workspace_format_step(
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Option<Step>, OrchestratorError> {
    let Some(fmt) = discovery.task_groups.iter().find(|group| {
        group.kind == TaskKind::Fmt && group.package_id.is_empty() && group.package_name.is_empty()
    }) else {
        return Ok(None);
    };
    let argv = crate::vectors::task_argv(fmt, catalog)?;
    let env = format_step_env(catalog)?;
    plan_format::format_step(argv, env)
        .map(Some)
        .map_err(OrchestratorError::from)
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

/// Gate MBX presence in crate jobs against their driver selection.
///
/// Cargo crates must be MBX-free; MBX crates carry exactly one
/// objects-mode step each. Jobs outside the driver map are unchecked.
///
/// # Errors
///
/// Returns a render error when MBX presence mismatches the driver.
pub(crate) fn check_crate_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), OrchestratorError> {
    check_mbx_gating(jobs, drivers).map_err(OrchestratorError::from)
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
    fn checkout_full_provides_history_for_git_archaeology() {
        let step = checkout_step_full().expect("full checkout step");
        let StepKind::Action { uses, with } = &step.kind else {
            panic!("checkout must be an action step");
        };
        assert_eq!(uses, &PinnedActionRef::checkout().uses_value());
        assert_eq!(
            with.get("fetch-depth").map(String::as_str),
            Some("0"),
            "plan checkout must clone full history: {with:?}"
        );
        assert!(validate_action_inputs(&checkout_inputs_schema(), with).is_ok());
        let shallow = checkout_step().expect("shallow checkout step");
        let StepKind::Action { with, .. } = &shallow.kind else {
            panic!("checkout must be an action step");
        };
        assert!(
            !with.contains_key("fetch-depth"),
            "non-plan checkouts stay shallow: {with:?}"
        );
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
    fn crate_tools_follow_selection_with_validators() {
        use velnor_actions_mise::PinnedTool;

        use crate::matrix_step::{prepare_crate_tools_step, task_driver_tools};
        assert_eq!(task_driver_tools(false), vec![PinnedTool::Rust]);
        assert_eq!(
            task_driver_tools(true),
            vec![PinnedTool::Rust, PinnedTool::MrBoxington]
        );
        let catalog = ToolCatalog::pinned();
        for (use_mbx, use_nextest) in [(false, false), (false, true), (true, false), (true, true)] {
            let step = prepare_crate_tools_step(&catalog, use_mbx, use_nextest).expect("step");
            let StepKind::Shell { run, .. } = &step.kind else {
                panic!("prepare must be a shell step");
            };
            for tool in [
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
            ] {
                assert!(
                    run.contains(&catalog.tool_spec(tool)),
                    "crate jobs install {tool:?} for test-spawned generate: {run:?}"
                );
            }
            let nextest = catalog.tool_spec(PinnedTool::Nextest);
            assert_eq!(run.contains(&nextest), use_nextest);
            let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
            assert_eq!(run.contains(&mbx), use_mbx);
            assert!(declared_config_variables().is_empty());
        }
    }
}
