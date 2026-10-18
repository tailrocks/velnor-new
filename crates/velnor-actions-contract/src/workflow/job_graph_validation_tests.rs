use std::collections::BTreeMap;

use crate::{
    ContractError,
    workflow::{Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr},
};

fn job(needs: &[&str]) -> Job {
    Job {
        display_name: "Graph test".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: vec![Step {
            name: "Run".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    }
}

fn workflow(jobs: &[(&str, &[&str])]) -> WorkflowIr {
    WorkflowIr {
        name: "Graph test".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "graph-test".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: jobs
            .iter()
            .map(|(id, needs)| ((*id).to_owned(), job(needs)))
            .collect(),
    }
}

#[test]
fn workflow_validation_accepts_an_acyclic_dependency_graph() {
    assert!(
        workflow(&[
            ("plan", &[]),
            ("check", &["plan"]),
            ("required", &["check"]),
        ])
        .validate()
        .is_ok()
    );
}

#[test]
fn workflow_validation_rejects_a_self_dependency() {
    assert_eq!(
        workflow(&[("check", &["check"])]).validate(),
        Err(ContractError::identity("workflow.jobs", "dependency_cycle"))
    );
}

#[test]
fn workflow_validation_rejects_a_non_preseed_multi_job_cycle() {
    assert_eq!(
        workflow(&[
            ("alpha", &["beta"]),
            ("beta", &["gamma"]),
            ("gamma", &["alpha"]),
        ])
        .validate(),
        Err(ContractError::identity("workflow.jobs", "dependency_cycle"))
    );
}
