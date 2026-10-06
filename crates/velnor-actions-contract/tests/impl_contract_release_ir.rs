//! Release IR cases: permissions, dispatch inputs, schedule, environment.
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::permissions::{PermissionLevel, Permissions};
use velnor_actions_contract::workflow::{
    Concurrency, DispatchInput, DispatchInputType, Job, Step, StepKind, Trigger, WorkflowDispatch,
    WorkflowIr,
};
use velnor_actions_contract::{ContractError, JobTimeout, ScheduleTrigger};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn input(name: &str, required: bool, default: Option<&str>) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        required,
        input_type: DispatchInputType::String,
        choices: Vec::new(),
        default: default.map(str::to_owned),
    }
}

fn identity_problem(workflow: &WorkflowIr) -> Option<String> {
    match workflow.validate() {
        Err(ContractError::InvalidIdentity { field, problem }) => {
            Some(format!("{field} {problem}"))
        }
        _ => None,
    }
}

fn ci_triggers() -> Trigger {
    Trigger {
        pull_request_types: vec!["opened".to_owned()],
        push_branches: vec!["main".to_owned()],
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn ci_job() -> Job {
    Job {
        display_name: "demo check".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec![],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: "run".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Internal {
                operation: "demo".to_owned(),
                env: std::collections::BTreeMap::new(),
            },
        }],
    }
}

fn ci_workflow() -> WorkflowIr {
    WorkflowIr {
        name: "demo CI".to_owned(),
        triggers: ci_triggers(),
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "demo".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: BTreeMap::from([("check".to_owned(), ci_job())]),
    }
}

fn check_job(workflow: &mut WorkflowIr) -> Option<&mut Job> {
    workflow.jobs.get_mut("check")
}

fn dispatch_inputs(workflow: &mut WorkflowIr) -> Option<&mut Vec<DispatchInput>> {
    workflow
        .triggers
        .workflow_dispatch
        .as_mut()
        .map(|dispatch| &mut dispatch.inputs)
}

#[test]
fn ir_default_permissions_grant_contents_only() {
    let permissions = Permissions::default();
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.actions, PermissionLevel::None);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert!(!permissions.is_write_all());
    assert_eq!(ci_workflow().validate(), Ok(()));
}

#[test]
fn ir_rejects_unsafe_push_branch_names() {
    for branch in ["main\non: [push]", "main'||true||'", "main.lock", "-main"] {
        let mut workflow = ci_workflow();
        workflow.triggers.push_branches = vec![branch.to_owned()];
        assert!(
            identity_problem(&workflow).is_some_and(|problem| {
                problem.starts_with("trigger.push_branches malformed_branch:")
            }),
            "unsafe branch name must fail IR validation: {branch:?}"
        );
    }
}

#[test]
fn ir_rejects_permission_violations() {
    let mut id_token = ci_workflow();
    id_token.permissions.id_token = PermissionLevel::Write;
    let got = identity_problem(&id_token).expect("must reject");
    assert_eq!(
        got,
        "job.environment id_token_write_needs_environment:check"
    );
    let mut bound = id_token.clone();
    check_job(&mut bound).expect("job").environment = Some("demo-publish".to_owned());
    bound.triggers.pull_request_types.clear();
    assert_eq!(bound.validate(), Ok(()));
    let mut on_pr = ci_workflow();
    let scoped = Permissions {
        contents: PermissionLevel::Write,
        ..Permissions::default()
    };
    check_job(&mut on_pr).expect("job").permissions = Some(scoped);
    let got = identity_problem(&on_pr).expect("must reject");
    assert_eq!(got, "job.permissions contents_write_on_pr:check");
    let write_all = Permissions {
        contents: PermissionLevel::Write,
        pull_requests: PermissionLevel::Write,
        id_token: PermissionLevel::Write,
        actions: PermissionLevel::Write,
    };
    let mut workflow_all = ci_workflow();
    workflow_all.permissions = write_all.clone();
    assert_eq!(
        identity_problem(&workflow_all).expect("must reject"),
        "workflow.permissions write_all"
    );
    let mut job_all = ci_workflow();
    check_job(&mut job_all).expect("job").permissions = Some(write_all);
    assert!(
        identity_problem(&job_all)
            .expect("must reject")
            .ends_with("write_all:check")
    );
}

#[test]
fn ir_validates_dispatch_input_charset_and_order() {
    assert_eq!(DispatchInputType::String.as_str(), "string");
    assert_eq!(DispatchInputType::Choice.as_str(), "choice");
    let mut workflow = ci_workflow();
    workflow.triggers.pull_request_types.clear();
    workflow.triggers.workflow_dispatch = Some(WorkflowDispatch {
        inputs: vec![
            input("package", true, None),
            input("source-sha", false, Some(SHA)),
        ],
    });
    assert_eq!(workflow.validate(), Ok(()));
    for name in ["", "Bad", "has space", "bad!", "bad/x", "UPPER"] {
        let mut bad = workflow.clone();
        dispatch_inputs(&mut bad).expect("dispatch")[0].name = name.to_owned();
        let got = identity_problem(&bad).expect("must reject");
        assert_eq!(got, format!("trigger.dispatch.inputs.name bad_name:{name}"));
    }
    let mut duplicate = workflow.clone();
    dispatch_inputs(&mut duplicate).expect("dispatch")[1].name = "package".to_owned();
    assert!(
        identity_problem(&duplicate)
            .expect("must reject")
            .ends_with("duplicate_input")
    );
    let mut unsorted = workflow.clone();
    *dispatch_inputs(&mut unsorted).expect("dispatch") =
        vec![input("zz", true, None), input("aa", true, None)];
    assert!(
        identity_problem(&unsorted)
            .expect("must reject")
            .ends_with("must_be_sorted")
    );
    let mut bad_default = workflow;
    dispatch_inputs(&mut bad_default).expect("dispatch")[1].default =
        Some("has\nnewline".to_owned());
    assert!(
        identity_problem(&bad_default)
            .expect("must reject")
            .ends_with("bad_default:source-sha")
    );
}

#[test]
fn ir_validates_choice_dispatch_inputs() {
    let mut workflow = ci_workflow();
    workflow.triggers.pull_request_types.clear();
    workflow.triggers.workflow_dispatch = Some(WorkflowDispatch {
        inputs: vec![DispatchInput {
            name: "phase".to_owned(),
            required: true,
            input_type: DispatchInputType::Choice,
            choices: vec!["cold".to_owned(), "warm".to_owned()],
            default: Some("cold".to_owned()),
        }],
    });
    assert_eq!(workflow.validate(), Ok(()));

    for choices in [
        vec![],
        vec!["warm".to_owned(), "cold".to_owned()],
        vec!["cold".to_owned(), "cold".to_owned()],
        vec!["cold".to_owned(), "Bad".to_owned()],
    ] {
        let mut bad = workflow.clone();
        dispatch_inputs(&mut bad).expect("dispatch")[0].choices = choices;
        assert!(identity_problem(&bad).is_some(), "{bad:?}");
    }
    let mut bad_default = workflow;
    dispatch_inputs(&mut bad_default).expect("dispatch")[0].default = Some("control".to_owned());
    assert!(identity_problem(&bad_default).is_some());
}

#[test]
fn ir_validates_schedule_and_environment_safety() {
    let mut scheduled = ci_workflow();
    scheduled.triggers.schedule = Some(ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned()],
    });
    assert_eq!(scheduled.validate(), Ok(()));
    let mut bad_cron = scheduled.clone();
    bad_cron.triggers.schedule.as_mut().expect("schedule").cron = vec!["nope".to_owned()];
    assert!(bad_cron.validate().is_err());
    let mut bad_env = scheduled;
    check_job(&mut bad_env).expect("job").environment = Some("../evil".to_owned());
    assert!(
        identity_problem(&bad_env)
            .expect("must reject")
            .ends_with("bad_environment:check")
    );
}
