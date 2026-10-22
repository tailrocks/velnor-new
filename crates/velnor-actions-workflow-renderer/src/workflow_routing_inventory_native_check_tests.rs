use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::{
    Concurrency, ExecutionMode, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr,
};

use super::{
    WorkflowRoutingPolicy, selector, validate_generated_workflow_calls,
    validate_generated_workflow_calls_with_ir, workflow,
};

#[test]
fn typed_native_check_needs_the_matching_ir_and_rendered_fields() {
    let selector = selector();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector, None);
    let workflow_ir = native_check_ir();
    let file = workflow(
        "ci",
        &format!(
            "on: push\njobs:\n  check-native:\n    runs-on: native-scale-set\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n    steps: []\n"
        ),
    );
    assert!(validate_generated_workflow_calls(std::slice::from_ref(&file), &policy).is_err());
    assert_eq!(
        validate_generated_workflow_calls_with_ir(
            std::slice::from_ref(&file),
            &policy,
            &workflow_ir,
        ),
        Ok(())
    );

    let altered = workflow(
        "ci",
        &format!(
            "on: push\njobs:\n  check-native:\n    runs-on: another-runner\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n    steps: []\n"
        ),
    );
    assert!(validate_generated_workflow_calls_with_ir(&[altered], &policy, &workflow_ir).is_err());
}

fn native_check_ir() -> WorkflowIr {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "check-native".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Check / native".to_owned(),
            runs_on: "native-scale-set".to_owned(),
            check_runner: Some(CheckRunner {
                label: "native-scale-set".to_owned(),
                platform: CheckPlatform::MacosArm64,
                executor: CheckExecutor::EphemeralSelfHosted,
                container: None,
            }),
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()),
            permissions: None,
            cache_mode: None,
            environment: None,
            steps: vec![Step {
                name: "Run native check".to_owned(),
                id: None,
                role: None,
                condition: None,
                parallel: false,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::new(),
                },
            }],
        },
    );
    WorkflowIr {
        name: "CI".to_owned(),
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
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs,
    }
}
