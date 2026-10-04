//! Release IR cases: permissions, dispatch inputs, schedule, environment.
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::ir::{
    Concurrency, DispatchInput, Job, Step, StepKind, Trigger, WorkflowDispatch, WorkflowIr,
};
use velnor_actions_contract::workflow::permissions::{PermissionLevel, Permissions};
use velnor_actions_contract::{ContractError, JobTimeout, ScheduleTrigger};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn input(name: &str, required: bool, default: Option<&str>) -> DispatchInput {
    DispatchInput {
        input_type: velnor_actions_contract::workflow::ir::DispatchInputType::String,
        description: None,
        options: Vec::new(),
        name: name.to_owned(),
        required,
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
        push_tags: Vec::new(),
        push_branches: vec!["main".to_owned()],
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn ci_job() -> Job {
    Job {
        cache_mode: None,
        display_name: "demo check".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: vec![],
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![Step {
            id: None,
            name: "run".to_owned(),
            condition: None,
            kind: StepKind::Internal {
                operation: "demo".to_owned(),
            },
        }],
    }
}

fn ci_workflow() -> WorkflowIr {
    WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        run_name: None,
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
fn ir_default_permissions_preserve_read_read_ci() {
    let permissions = Permissions::default();
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.actions, PermissionLevel::Read);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert!(!permissions.is_write_all());
    assert_eq!(ci_workflow().validate(), Ok(()));
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
    assert!(bound.validate().is_err());
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
        issues: PermissionLevel::Write,
        pages: PermissionLevel::None,
        attestations: PermissionLevel::None,
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
    assert_eq!(
        velnor_actions_contract::workflow::ir::DispatchInputType::String.as_str(),
        "string"
    );
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

#[test]
fn dispatch_boolean_defaults_validate_and_legacy_reads_default_to_string() {
    use velnor_actions_contract::workflow::ir::DispatchInputType;
    let old: DispatchInput =
        serde_json::from_str(r#"{"name":"scope","required":false,"default":"full"}"#)
            .expect("old input");
    assert_eq!(old.input_type, DispatchInputType::String);
    let permissions: Permissions = serde_json::from_str(
        r#"{"contents":"read","pull_requests":"none","id_token":"none","actions":"read"}"#,
    )
    .expect("old permissions");
    assert_eq!(permissions.issues, PermissionLevel::None);
    for value in ["true", "false", "False", "1", "${{ github.token }}"] {
        let mut input = input("simulate_failure", false, Some(value));
        input.input_type = DispatchInputType::Boolean;
        let mut workflow = ci_workflow();
        workflow.triggers.workflow_dispatch = Some(WorkflowDispatch {
            inputs: vec![input],
        });
        assert_eq!(
            workflow.validate().is_ok(),
            matches!(value, "true" | "false"),
            "{value}"
        );
    }
}

#[test]
fn native_pages_permissions_cannot_escalate_ci_jobs() {
    let mut workflow = ci_workflow();
    workflow.permissions.pages = PermissionLevel::Read;
    assert_eq!(workflow.validate(), Ok(()));
    workflow.permissions.pages = PermissionLevel::Write;
    assert!(workflow.validate().is_err());
    workflow.permissions.pages = PermissionLevel::None;
    check_job(&mut workflow).expect("job").permissions = Some(Permissions {
        pages: PermissionLevel::Write,
        attestations: PermissionLevel::None,
        ..Permissions::default()
    });
    assert!(workflow.validate().is_err());
    workflow.triggers.pull_request_types.clear();
    check_job(&mut workflow).expect("job").environment = Some("github-pages".to_owned());
    assert!(workflow.validate().is_err());
}

#[test]
fn native_oidc_requires_an_approved_closed_role_even_with_environment() {
    for environment in ["arbitrary", "github-pages", "package-feed"] {
        let mut workflow = ci_workflow();
        let job = check_job(&mut workflow).expect("job");
        job.environment = Some(environment.to_owned());
        job.permissions = Some(Permissions {
            id_token: PermissionLevel::Write,
            ..Permissions::default()
        });
        let error = identity_problem(&workflow).expect("must reject");
        assert!(
            error.contains("id_token_write_needs_closed_role"),
            "{error}"
        );
    }
}
