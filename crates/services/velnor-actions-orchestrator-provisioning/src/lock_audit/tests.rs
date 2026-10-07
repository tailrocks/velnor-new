//! Install-audit glue tests: extraction failures block, labels gate loudly.
use std::collections::BTreeMap;

use velnor_actions_contract_config::config::ValidatorKind;
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, StepRole, Trigger, WorkflowIr,
};
use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;
use velnor_actions_workflow_jobs::context::ValidatorCommand;
use velnor_actions_workflow_steps::steps::DENY_STEP_NAME;

use super::audit_prepare_installs;
use crate::vectors::CARGO_DENY_VERSION;

fn shell_job(run: Vec<String>) -> Job {
    Job {
        check_runner: None,
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
            id: None,
            role: Some(StepRole::PreparePinnedTools),
            condition: None,
            kind: StepKind::Shell {
                run,
                env: BTreeMap::new(),
            },
        }],
    }
}

/// Deny privilege-drop command shape: `sh -c` over install plus payload.
fn deny_command(script: &str) -> ValidatorCommand {
    ValidatorCommand {
        validator: ValidatorKind::CargoDeny,
        name: DENY_STEP_NAME.to_owned(),
        argv: vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
    }
}

fn ir_for(job: Job) -> WorkflowIr {
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
        jobs: BTreeMap::from([("plan".to_owned(), job)]),
    }
}

mod lock_audit_tests;
