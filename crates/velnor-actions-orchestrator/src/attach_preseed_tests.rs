//! Pre-seed MBX identity and ordering tests.

use std::collections::BTreeMap;

use super::attach_tests::bare_ir;
use super::{MBX_ACTION_NAME, attach_preseed, preseed_anchor};
use crate::workflow::{CHECKOUT_USES, REQUEST_DIR, WorkflowPlan};
use crate::workflow_jobs::{final_job, plan_job};
use velnor_actions_contract::Step;
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, ToolCatalog};
use velnor_actions_workflow_renderer::cache_p08::RESTORE_SOURCES_NAME;
use velnor_actions_workflow_renderer::render::{RenderContext, WORKFLOW_PATH};
use velnor_actions_workflow_renderer::steps::{MBX_SETUP_NAME, STAGED_BINARY_PREFIX};

/// Pre-seed fixture plan over one plan job plus the final gate.
fn preseed_fixture(use_mbx: bool, fetch_roots: &[String]) -> WorkflowPlan {
    use velnor_actions_actionlint::ActionlintConfigInput;
    let catalog = ToolCatalog::pinned();
    WorkflowPlan {
        ir: bare_ir(BTreeMap::from([
            (
                "plan".to_owned(),
                plan_job(
                    "ubuntu-26.04",
                    None,
                    &catalog,
                    true,
                    use_mbx,
                    false,
                    false,
                    fetch_roots,
                )
                .expect("plan job"),
            ),
            (
                "required".to_owned(),
                final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
            ),
        ])),
        support: None,
        context: RenderContext {
            generator_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            staged_binary: format!("{STAGED_BINARY_PREFIX}0.1.0"),
            request_dir: REQUEST_DIR.to_owned(),
            checkout_uses: CHECKOUT_USES.to_owned(),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
            plan_consumer_env: BTreeMap::new(),
        },
        actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
    }
}

/// Assert one plan step runs under the owned fetch homes.
fn assert_owned_homes(steps: &[Step], name: &str) {
    let step = steps
        .iter()
        .find(|step| step.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("{name} must be a shell step");
    };
    for key in ["MISE_CARGO_HOME", "MISE_RUSTUP_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(env.contains_key(key), "{name} misses {key}: {env:?}");
    }
}

#[test]
fn preseed_mbx_setup_and_build_follow_sources_with_homes() {
    use velnor_actions_workflow_renderer::cache_p08::SAVE_SOURCES_NAME;
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let mut plan = preseed_fixture(true, &[String::new()]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| {
        names
            .iter()
            .position(|step| *step == name)
            .unwrap_or_else(|| panic!("missing {name}: {names:?}"))
    };
    let (restore, mbx, probe, build, verify, save) = (
        at(RESTORE_SOURCES_NAME),
        at(MBX_SETUP_NAME),
        at(crate::source_prep::FETCH_SOURCES_STEP),
        at(PRESEED_BUILD_NAME),
        at(PRESEED_VERIFY_NAME),
        at(SAVE_SOURCES_NAME),
    );
    assert!(
        restore < mbx && mbx < probe && probe < build && build < verify && verify < save,
        "preseed order: {names:?}"
    );
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}

#[test]
fn preseed_skips_mbx_setup_for_cargo_only_plans() {
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let mut plan = preseed_fixture(false, &[String::new()]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !names.contains(&MBX_SETUP_NAME),
        "cargo-only plans stay rust-cache-only: {names:?}"
    );
    let probe = names
        .iter()
        .position(|step| *step == crate::source_prep::FETCH_SOURCES_STEP)
        .expect("sources step");
    let build = names
        .iter()
        .position(|step| *step == PRESEED_BUILD_NAME)
        .expect("build step");
    assert!(probe < build, "build anchors after sources: {names:?}");
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}

#[test]
fn preseed_anchor_uses_mbx_action_identity() {
    let steps = vec![
        Step {
            name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
            condition: None,
            kind: velnor_actions_contract::StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        },
        Step {
            name: "Install MBX runtime".to_owned(),
            condition: None,
            kind: velnor_actions_contract::StepKind::Action {
                uses: format!("{MBX_ACTION_NAME}@{}", "d".repeat(40)),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        },
        Step {
            name: MBX_SETUP_NAME.to_owned(),
            condition: None,
            kind: velnor_actions_contract::StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        },
    ];
    assert_eq!(preseed_anchor(&steps), 2);
}
