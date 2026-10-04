use super::{WorkflowDocumentContext, render_workflow_document};
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::{
    ir::{Concurrency, Job, Trigger, WorkflowIr},
    permissions::Permissions,
    step::{Step, StepKind},
    timeout::JobTimeout,
};

fn fixture() -> (WorkflowIr, WorkflowDocumentContext) {
    let job = Job {
        cache_mode: None,
        display_name: "Upload".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: vec![Step {
            id: None,
            name: "Upload".to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses: "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }],
    };
    let ir = WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        name: "Native".to_owned(),
        run_name: Some("Native run".to_owned()),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            push_tags: vec!["v[0-9]*".to_owned()],
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "native".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: BTreeMap::from([("upload".to_owned(), job)]),
    };
    let ctx = WorkflowDocumentContext {
        generator_version: "0.1.0".to_owned(),
        source_helpers: Vec::new(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    (ir, ctx)
}

#[test]
fn native_document_has_only_declared_events_and_no_ci_mutation() {
    let (ir, ctx) = fixture();
    let text = render_workflow_document(&ir, &ctx).expect("document");
    assert!(text.contains("run-name: Native run"));
    assert!(text.contains("cancel-in-progress: false"));
    assert!(!text.contains("cancel-in-progress: \"false\""));
    assert!(text.contains("tags:"));
    assert!(text.contains("v[0-9]*"));
    for forbidden in [
        "pull_request:",
        "branches:",
        "merge_group:",
        "plan-v1",
        "aggregate-v1",
        "final-v1",
        "request.json",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn native_document_rejects_raw_operations_and_invalid_tag_patterns() {
    let (mut ir, ctx) = fixture();
    ir.jobs.get_mut("upload").expect("job").steps[0].kind = StepKind::Internal {
        operation: "plan-v1".to_owned(),
    };
    assert!(render_workflow_document(&ir, &ctx).is_err());
    let (mut ir, ctx) = fixture();
    ir.jobs.get_mut("upload").expect("job").steps[0].kind = StepKind::Shell {
        run: vec!["true".to_owned()],
        env: BTreeMap::new(),
    };
    assert!(render_workflow_document(&ir, &ctx).is_err());
    for tags in [
        vec!["${{ github.ref }}"],
        vec!["*"],
        vec!["v*", "v*"],
        vec!["v*", "v[0-9]*"],
    ] {
        let (mut ir, ctx) = fixture();
        ir.triggers.push_tags = tags.into_iter().map(str::to_owned).collect();
        assert!(render_workflow_document(&ir, &ctx).is_err());
    }
}
