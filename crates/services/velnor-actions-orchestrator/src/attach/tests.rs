use super::*;
use crate::workflow::{CHECKOUT_USES, REQUEST_DIR};
use crate::workflow_jobs::{final_job, plan_job};
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::{Concurrency, Permissions, Trigger};
use velnor_actions_workflow_jobs::context::RenderContext;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

/// Assert one plan step runs under the owned fetch homes.
fn assert_owned_homes(
    steps: &[Step],
    role: velnor_actions_contract_workflow::StepRole,
    name: &str,
) {
    let step = steps
        .iter()
        .find(|step| step.role == Some(role))
        .unwrap_or_else(|| panic!("missing {role:?} ({name})"));
    let velnor_actions_contract_workflow::StepKind::Shell { env, .. } = &step.kind else {
        panic!("{name} must be a shell step");
    };
    for key in ["MISE_CARGO_HOME", "MISE_RUSTUP_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(env.contains_key(key), "{name} misses {key}: {env:?}");
    }
}
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
            verification_tasks: Vec::new(),
            plan_consumer_env: BTreeMap::new(),
        },
        actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
    }
}

/// Bare IR shell shared by the attach fixtures.
fn bare_ir(jobs: BTreeMap<String, velnor_actions_contract_workflow::Job>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "g".to_owned(),
            cancel_in_progress: "c".to_owned(),
        },
        jobs,
    }
}

mod attach_tests;
mod mbx_tests;
mod source_cache_tests;
