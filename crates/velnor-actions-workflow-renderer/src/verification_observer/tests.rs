use super::*;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, PermissionLevel, Permissions, Trigger, WorkflowIr,
};

fn workflow() -> WorkflowIr {
    let observer = super::tests::observer_jobs()
        .remove("verification-observer")
        .expect("observer fixture");
    let required = Job {
        cache_mode: None,
        display_name: "Required".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: vec![],
        condition: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        permissions: None,
        steps: vec![
            crate::shell_step("Check", vec!["true".to_owned()], BTreeMap::new()).expect("step"),
        ],
    };
    WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        run_name: None,
        name: "CI".to_owned(),
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "CI".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_tags: Vec::new(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: Some(velnor_actions_contract::ScheduleTrigger {
                cron: vec!["17 3 * * *".to_owned()],
            }),
        },
        jobs: BTreeMap::from([
            ("verification-observer".to_owned(), observer),
            (crate::render::FINAL_JOB_ID.to_owned(), required),
        ]),
    }
}

#[test]
fn observer_contract_accepts_schedule_only_and_fixed_payload() {
    let workflow = workflow();
    workflow.validate().expect("closed observer contract");
    validate_observer_jobs(
        &workflow.jobs,
        &workflow.triggers,
        &super::tests::records(),
        env!("CARGO_PKG_VERSION"),
    )
    .expect("fixed payload");
}

#[test]
fn observer_permission_gate_and_payload_fail_closed() {
    let original = workflow();
    let mut workflow = original.clone();
    workflow.permissions.issues = PermissionLevel::Write;
    assert!(workflow.validate().is_err());
    for mutation in [
        "condition",
        "shell",
        "action",
        "wrong-op",
        "args",
        "bad-binding",
        "missing-binding",
        "scope",
    ] {
        let mut workflow = original.clone();
        let observer = workflow
            .jobs
            .get_mut("verification-observer")
            .expect("observer");
        match mutation {
            "condition" => observer.condition = Some("always()".to_owned()),
            "shell" => {
                observer.steps[1].kind = StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::new(),
                };
            }
            "action" => {
                observer.steps[0].kind = StepKind::Action {
                    uses: "owner/action@sha".to_owned(),
                    with: BTreeMap::new(),
                    env: BTreeMap::new(),
                };
            }
            "wrong-op" => observer.steps[0] = observer.steps[1].clone(),
            "args" => {
                let record = super::tests::observer_record_with_args(
                    "tailrocks/velnor-new",
                    "main",
                    vec!["unexpected".to_owned()],
                );
                observer.steps[2] = crate::source_helper::source_helper_step(
                    "Open or update nightly failure signal",
                    &record,
                    record.environment().clone(),
                )
                .expect("malformed observer helper");
            }
            "missing-binding" => {
                if let StepKind::SourceBoundHelper { env, .. } = &mut observer.steps[2].kind {
                    env.remove("SOURCE_SHA");
                }
            }
            "bad-binding" => {
                if let StepKind::SourceBoundHelper { env, .. } = &mut observer.steps[2].kind {
                    env.insert("SOURCE_SHA".to_owned(), "bad".to_owned());
                }
            }
            _ => {
                observer.permissions.as_mut().expect("permissions").contents = PermissionLevel::Read
            }
        }
        assert!(
            workflow.validate().is_err()
                || validate_observer_jobs(
                    &workflow.jobs,
                    &workflow.triggers,
                    &super::tests::records(),
                    env!("CARGO_PKG_VERSION"),
                )
                .is_err(),
            "{mutation}"
        );
    }
}
