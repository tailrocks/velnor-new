// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

use velnor_actions_contract::{
    CacheMode, Concurrency, DispatchInput, DispatchInputType, Job, JobTimeout, PermissionLevel,
    Permissions, REQUIRED_CONDITION, REQUIRED_JOB_ID, ScheduleTrigger, Step, StepKind, Trigger,
    WorkflowDispatch, WorkflowIr,
};
use velnor_actions_workflow_renderer::steps::{
    BOOTSTRAP_MAX_BYTES, BOOTSTRAP_SHADOW_CLEAR, BootstrapBridge,
};

pub(crate) fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

pub(crate) fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

pub(crate) fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

pub(crate) fn shell_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "printf ok".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

pub(crate) fn job_with(
    display: &str,
    cache_mode: Option<CacheMode>,
    permissions: Option<Permissions>,
    needs: Vec<String>,
    condition: Option<String>,
) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs,
        condition,
        permissions,
        cache_mode,
        environment: None,
        services: Vec::new(),
        steps: vec![shell_step(display)],
    }
}

pub(crate) fn cleanup_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Write,
        packages: PermissionLevel::None,
    }
}

pub(crate) fn mode_jobs(writer_mode: Option<CacheMode>) -> BTreeMap<String, Job> {
    BTreeMap::from([
        (
            "reader".to_owned(),
            job_with(
                "Reader",
                Some(CacheMode::explicit_read()),
                None,
                Vec::new(),
                None,
            ),
        ),
        (
            "writer".to_owned(),
            job_with("Writer", writer_mode, None, Vec::new(), None),
        ),
        (
            "cleanup".to_owned(),
            job_with(
                "Cleanup",
                None,
                Some(cleanup_permissions()),
                Vec::new(),
                None,
            ),
        ),
        (
            REQUIRED_JOB_ID.to_owned(),
            job_with(
                "Required",
                None,
                None,
                vec!["writer".to_owned(), "reader".to_owned()],
                Some(REQUIRED_CONDITION.to_owned()),
            ),
        ),
    ])
}

pub(crate) fn mode_workflow(trigger: Trigger, writer_mode: Option<CacheMode>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: trigger,
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "velnor-${{ github.workflow }}-${{ github.ref }}".to_owned(),
            cancel_in_progress: "${{ github.event_name == 'pull_request' }}".to_owned(),
        },
        jobs: mode_jobs(writer_mode),
    }
}

pub(crate) fn pr_trigger() -> Trigger {
    Trigger {
        pull_request_types: vec!["opened".to_owned(), "synchronize".to_owned()],
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

pub(crate) fn push_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: vec!["main".to_owned()],
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

pub(crate) fn merge_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: Vec::new(),
        merge_group: true,
        workflow_dispatch: None,
        schedule: None,
    }
}

pub(crate) fn dispatch_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: Some(WorkflowDispatch {
            inputs: vec![DispatchInput {
                name: "environment".to_owned(),
                required: true,
                input_type: DispatchInputType::String,
                choices: Vec::new(),
                default: None,
            }],
        }),
        schedule: None,
    }
}

pub(crate) fn schedule_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: None,
        schedule: Some(ScheduleTrigger::single("0 6 * * 1")),
    }
}

pub(crate) fn names<'a>(values: &'a [&'a str]) -> BTreeSet<&'a str> {
    values.iter().copied().collect()
}

pub(crate) fn branch_jobs() -> BTreeMap<String, Job> {
    BTreeMap::from([
        (
            "download".to_owned(),
            job_with("Download", None, None, Vec::new(), None),
        ),
        (
            "image".to_owned(),
            job_with("Image", None, None, Vec::new(), None),
        ),
        (
            "join".to_owned(),
            job_with(
                "Join",
                None,
                None,
                Vec::new(),
                Some(REQUIRED_CONDITION.to_owned()),
            ),
        ),
    ])
}

pub(crate) fn guarded_bridge() -> BootstrapBridge {
    BootstrapBridge {
        payload_dir: "${{ runner.temp }}/velnor-gc1/transport/tools".to_owned(),
        member: "velnor-controller/velnor-actions".to_owned(),
        staged_path: "${{ runner.temp }}/velnor-gc1/staged/velnor-actions".to_owned(),
        max_bytes: BOOTSTRAP_MAX_BYTES,
        expected_key: Some("velnor-gc1-tools-group-compatibility-coverage-r0".to_owned()),
        matched_key: Some("velnor-gc1-tools-group-compatibility-coverage-r0".to_owned()),
        shadow_state: Some(BOOTSTRAP_SHADOW_CLEAR.to_owned()),
    }
}

pub(crate) fn checkout_step() -> Step {
    Step {
        name: "Checkout".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}
