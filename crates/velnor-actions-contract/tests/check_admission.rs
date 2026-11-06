// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::workflow::{
    CacheAccess, Concurrency, Job, JobEnv, JobTimeout, PermissionLevel, Permissions, Step,
    StepKind, Trigger, WorkflowIr, not_affected_gate,
};

fn workflow(condition: &str, runs_on: &str, runner: CheckRunner) -> WorkflowIr {
    let plan = Job {
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_access: CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: vec![step()],
    };
    let check = Job {
        display_name: "Maintenance check".to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: Some(runner),
        timeout_minutes: JobTimeout::DEFAULT,
        needs: vec!["plan".to_owned()],
        condition: Some(condition.to_owned()),
        permissions: None,
        cache_access: CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: vec![step()],
    };
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions {
            contents: PermissionLevel::Read,
            ..Permissions::default()
        },
        concurrency: Concurrency::writer_queue("ci"),
        jobs: BTreeMap::from([
            ("plan".to_owned(), plan),
            ("check-maintenance-helpers".to_owned(), check),
        ]),
    }
}

fn step() -> Step {
    Step {
        name: "Run".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn scale_set_runner() -> CheckRunner {
    CheckRunner {
        label: "ubuntu-24.04".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

#[test]
fn validation_accepts_canonical_scale_set_gate_and_bare_admission() {
    let id = "check-maintenance-helpers";
    let condition = format!(
        "({}) && ({})",
        not_affected_gate(id),
        EPHEMERAL_CHECK_ADMISSION_CONDITION
    );
    let scale_set = workflow(
        &condition,
        "scale-set:velnor+ubuntu-26.04-scale-set",
        scale_set_runner(),
    );
    assert!(scale_set.validate().is_ok());

    let ephemeral = workflow(
        EPHEMERAL_CHECK_ADMISSION_CONDITION,
        "velnor-private-linux",
        CheckRunner {
            label: "velnor-private-linux".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::EphemeralSelfHosted,
            container: None,
        },
    );
    assert!(ephemeral.validate().is_ok());
}

#[test]
fn validation_rejects_or_prefixed_scale_set_admission_bypass() {
    let admission = EPHEMERAL_CHECK_ADMISSION_CONDITION;
    let condition = format!("true || anything && ({admission})");
    let candidate = workflow(
        &condition,
        "scale-set:velnor+ubuntu-26.04-scale-set",
        scale_set_runner(),
    );
    assert!(matches!(
        candidate.validate(),
        Err(ContractError::InvalidIdentity {
            field: "job.runs_on",
            problem,
        }) if problem == "check_runner_scale_set_mismatch"
    ));
}
