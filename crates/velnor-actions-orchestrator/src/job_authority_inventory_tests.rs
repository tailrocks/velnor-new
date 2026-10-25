use std::collections::BTreeMap;

use velnor_actions_contract::config::{JobAuthorityBinding, JobAuthorityRole};
use velnor_actions_workflow_renderer::{
    AuthorityCall, AuthorityConcurrency, AuthorityJob, AuthorityPermissions, AuthorityTrigger,
    AuthorityWorkflow, RenderedFile,
};

use super::build_from_facts;

const ROOT: &str = ".github/workflows/ci.yml";
const CALLEE: &str = ".github/workflows/reusable.yml";

fn trigger() -> AuthorityTrigger {
    AuthorityTrigger {
        event: "push".to_owned(),
        branch_filters: vec!["sea/integration".to_owned()],
        tag_filters: Vec::new(),
        pull_request_base_branches: Vec::new(),
    }
}

fn job(key: &str, reusable_call: Option<&str>) -> AuthorityJob {
    AuthorityJob {
        job_key: key.to_owned(),
        needs: Vec::new(),
        permissions: AuthorityPermissions {
            declared: true,
            scopes: BTreeMap::from([("contents".to_owned(), "read".to_owned())]),
        },
        secrets: Vec::new(),
        environments: Vec::new(),
        condition: Some("github.event_name == 'push'".to_owned()),
        concurrency: Some(AuthorityConcurrency {
            scope: "workflow".to_owned(),
            group: "ci-${{ github.ref }}".to_owned(),
            cancel_in_progress: "false".to_owned(),
        }),
        has_matrix: false,
        reusable_call: reusable_call.map(str::to_owned),
    }
}

fn input() -> (
    Vec<AuthorityWorkflow>,
    Vec<RenderedFile>,
    Vec<JobAuthorityBinding>,
) {
    let facts = vec![
        AuthorityWorkflow {
            path: ROOT.to_owned(),
            triggers: vec![trigger()],
            calls: vec![AuthorityCall {
                job_key: "build".to_owned(),
                path: CALLEE.to_owned(),
            }],
            workflow_permissions: Some(BTreeMap::from([(
                "contents".to_owned(),
                "read".to_owned(),
            )])),
            workflow_concurrency: None,
            jobs: vec![job("build", Some(CALLEE))],
        },
        AuthorityWorkflow {
            path: CALLEE.to_owned(),
            triggers: Vec::new(),
            calls: Vec::new(),
            workflow_permissions: Some(BTreeMap::from([(
                "contents".to_owned(),
                "read".to_owned(),
            )])),
            workflow_concurrency: None,
            jobs: vec![job("test", None)],
        },
    ];
    let files = vec![
        RenderedFile {
            path: ROOT.to_owned(),
            bytes: "workflow root".to_owned(),
        },
        RenderedFile {
            path: CALLEE.to_owned(),
            bytes: "workflow callee".to_owned(),
        },
    ];
    let bindings = vec![
        JobAuthorityBinding {
            root_path: ROOT.to_owned(),
            workflow_path: ROOT.to_owned(),
            job_key: "build".to_owned(),
            role: JobAuthorityRole::Verification,
        },
        JobAuthorityBinding {
            root_path: ROOT.to_owned(),
            workflow_path: CALLEE.to_owned(),
            job_key: "test".to_owned(),
            role: JobAuthorityRole::TrustedPublication,
        },
    ];
    (facts, files, bindings)
}

fn single_workflow_input() -> (
    Vec<AuthorityWorkflow>,
    Vec<RenderedFile>,
    Vec<JobAuthorityBinding>,
) {
    let (mut facts, mut files, mut bindings) = input();
    facts.truncate(1);
    facts[0].calls.clear();
    facts[0].jobs[0].reusable_call = None;
    files.truncate(1);
    bindings.truncate(1);
    (facts, files, bindings)
}

#[test]
fn builds_closed_inventory_from_one_workflow() {
    let (facts, files, bindings) = single_workflow_input();
    let inventory = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect("closed authority inventory");
    let repeated = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect("repeated authority inventory");
    assert_eq!(inventory, repeated);
    assert_eq!(inventory.workflows.len(), 1);
    assert_eq!(inventory.workflows[0].calls.len(), 0);
    assert_eq!(
        inventory.workflows[0].jobs[0].role,
        JobAuthorityRole::Verification
    );
    assert_eq!(inventory.inventory_sha256.len(), 64);
}

#[test]
fn rejects_reusable_workflow_authority_composition() {
    let (facts, files, bindings) = input();
    let error = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect_err("reusable authority composition must fail closed");
    assert!(
        error
            .to_string()
            .contains("reusable_workflow_authority_composition_unsupported")
    );
}

#[test]
fn rejects_jobs_without_declared_permissions() {
    let (mut facts, files, bindings) = single_workflow_input();
    facts[0].jobs[0].permissions.declared = false;
    let error = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect_err("undeclared permissions must fail");
    assert!(error.to_string().contains("job_permissions_undeclared"));
}

#[test]
fn rejects_missing_and_unmatched_roles() {
    let (facts, files, mut bindings) = single_workflow_input();
    bindings.pop();
    let error = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect_err("missing role must fail");
    assert!(error.to_string().contains("job_role_missing"));

    let (facts, files, mut bindings) = single_workflow_input();
    bindings.push(JobAuthorityBinding {
        root_path: ROOT.to_owned(),
        workflow_path: ROOT.to_owned(),
        job_key: "extra".to_owned(),
        role: JobAuthorityRole::Verification,
    });
    let error = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect_err("unmatched role must fail");
    assert!(error.to_string().contains("job_role_binding_unmatched"));
}

#[test]
fn rejects_workflows_outside_trigger_closure() {
    let (mut facts, mut files, bindings) = single_workflow_input();
    let orphan = ".github/workflows/orphan.yml";
    facts.push(AuthorityWorkflow {
        path: orphan.to_owned(),
        triggers: Vec::new(),
        calls: Vec::new(),
        workflow_permissions: None,
        workflow_concurrency: None,
        jobs: Vec::new(),
    });
    files.push(RenderedFile {
        path: orphan.to_owned(),
        bytes: "orphan workflow".to_owned(),
    });
    let error = build_from_facts(&facts, &files, &bindings, "a".repeat(40))
        .expect_err("orphan workflow must fail");
    assert!(
        error
            .to_string()
            .contains("workflow_outside_trigger_closure")
    );
}
