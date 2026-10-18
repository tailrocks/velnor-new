use std::collections::BTreeMap;

use velnor_actions_contract::{
    CheckExecutor, CheckPlatform, CheckRunner, Concurrency, Job, JobTimeout, Permissions,
    SCALE_SET_NAME, ScaleSetSelector, Trigger, VELNOR_LABEL, WorkflowIr,
};

use super::check_single_label;
use crate::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP};

fn scale_set_label() -> String {
    ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("valid selector")
    .token()
}

fn check_workflow(
    id: &str,
    runs_on: &str,
    runner_label: &str,
    platform: CheckPlatform,
    executor: CheckExecutor,
    condition: Option<&str>,
) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::from([(
            id.to_owned(),
            Job {
                display_name: "Check / runner policy".to_owned(),
                runs_on: runs_on.to_owned(),
                check_runner: Some(CheckRunner {
                    label: runner_label.to_owned(),
                    platform,
                    executor,
                    container: None,
                }),
                timeout_minutes: JobTimeout::CRATE,
                needs: Vec::new(),
                condition: condition.map(str::to_owned),
                permissions: None,
                cache_mode: None,
                environment: None,
                steps: Vec::new(),
            },
        )]),
    }
}

fn accepts(id: &str, runs_on: &str, condition: Option<&str>) -> bool {
    check_single_label(
        &check_workflow(
            id,
            runs_on,
            "ubuntu-26.04",
            CheckPlatform::LinuxX64,
            CheckExecutor::Hosted,
            condition,
        ),
        "ubuntu-26.04",
        &[],
    )
    .is_ok()
}

#[test]
fn hosted_check_jobs_keep_their_labels_in_hosted_and_both_modes() {
    for id in [
        "check-system",
        "check-system-hosted",
        "qualification-check-system",
    ] {
        assert!(accepts(id, "ubuntu-26.04", None), "{id}");
    }
}

#[test]
fn qualification_scale_set_check_requires_its_lane_admission_and_linux_hosted_runner() {
    let scale = scale_set_label();
    let valid = check_workflow(
        "qualification-check-maintenance-helpers",
        &scale,
        "ubuntu-26.04",
        CheckPlatform::LinuxX64,
        CheckExecutor::Hosted,
        Some(velnor_actions_contract::config::QUALIFICATION_CHECK_ADMISSION_CONDITION),
    );
    assert!(check_single_label(&valid, "ubuntu-26.04", &[]).is_ok());

    for condition in [
        None,
        Some(velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION),
        Some(velnor_actions_contract::config::NORMAL_EPHEMERAL_CHECK_ADMISSION_CONDITION),
    ] {
        let wrong_condition = check_workflow(
            "qualification-check-maintenance-helpers",
            &scale,
            "ubuntu-26.04",
            CheckPlatform::LinuxX64,
            CheckExecutor::Hosted,
            condition,
        );
        assert!(check_single_label(&wrong_condition, "ubuntu-26.04", &[]).is_err());
    }

    for (platform, executor) in [
        (CheckPlatform::MacosX64, CheckExecutor::Hosted),
        (CheckPlatform::LinuxX64, CheckExecutor::EphemeralSelfHosted),
    ] {
        let wrong_runner = check_workflow(
            "qualification-check-maintenance-helpers",
            &scale,
            "ubuntu-26.04",
            platform,
            executor,
            Some(velnor_actions_contract::config::QUALIFICATION_CHECK_ADMISSION_CONDITION),
        );
        assert!(check_single_label(&wrong_runner, "ubuntu-26.04", &[]).is_err());
    }
}

#[test]
fn ordinary_scale_set_checks_keep_both_existing_admission_conditions() {
    let scale = scale_set_label();
    for condition in [
        velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION,
        velnor_actions_contract::config::NORMAL_EPHEMERAL_CHECK_ADMISSION_CONDITION,
    ] {
        let workflow = check_workflow(
            "check-maintenance-helpers",
            &scale,
            "ubuntu-26.04",
            CheckPlatform::LinuxX64,
            CheckExecutor::Hosted,
            Some(condition),
        );
        assert!(check_single_label(&workflow, "ubuntu-26.04", &[]).is_ok());
    }
    assert!(!accepts(
        "check-maintenance-helpers",
        &scale,
        Some(velnor_actions_contract::config::QUALIFICATION_CHECK_ADMISSION_CONDITION),
    ));
}

#[test]
fn runner_contract_is_rejected_for_unrecognized_job_ids() {
    assert!(!accepts("maintenance-helpers", "ubuntu-26.04", None));
}
