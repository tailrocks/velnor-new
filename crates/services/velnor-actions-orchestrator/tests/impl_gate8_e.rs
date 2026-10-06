//! Regression test E: candidate never plans; lock matches catalog per target.
use std::collections::BTreeMap;

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_release::SUPPORTED_TARGETS;
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Step, Trigger, WorkflowIr,
};
use velnor_actions_mise::catalog::lock::{
    parse_generator_lock, parse_release_manifest, verify_lock_against_manifest,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, checkout_step, plan_step,
    render_workflow_ir,
};

use crate::impl_common::TestResult;

const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

fn binary_record(target: &str, sha: &str) -> String {
    format!(
        "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://github.com/tailrocks/velnor-new/releases/download/v{GENERATOR_VERSION}/velnor-actions-{GENERATOR_VERSION}-{target}\"\nsha256 = \"{sha}\"\n"
    )
}

fn lock_text(sha: &str) -> String {
    let bins = SUPPORTED_TARGETS
        .iter()
        .map(|target| binary_record(target, sha))
        .collect::<String>();
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{GENERATOR_VERSION}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "a".repeat(40),
        "c".repeat(64)
    )
}

fn manifest_text(sha: &str) -> String {
    let targets = velnor_actions_contract_release::SUPPORTED_TARGETS
        .iter()
        .map(|t| format!("{{\"target\":\"{t}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{GENERATOR_VERSION}/velnor-actions-{GENERATOR_VERSION}-{t}\",\"sha256\":\"{sha}\"}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{GENERATOR_VERSION}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "a".repeat(40)
    )
}

fn ctx() -> RenderContext {
    RenderContext {
        generator_version: GENERATOR_VERSION.to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{GENERATOR_VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

fn ir_with(steps: Vec<Step>) -> Result<WorkflowIr, Box<dyn std::error::Error>> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "plan".to_owned(),
        Job {
            check_runner: None,
            display_name: "P".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![
                checkout_step(&format!("actions/checkout@{:040x}", 0))?,
                plan_step(),
            ],
        },
    );
    jobs.insert(
        "candidate".to_owned(),
        Job {
            check_runner: None,
            display_name: "C".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: vec!["plan".to_owned()],
            condition: None,
            permissions: None,
            environment: None,
            steps,
        },
    );
    Ok(WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .iter()
                .map(ToString::to_string)
                .collect(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    })
}

#[test]
fn candidate_never_plans_and_lock_matches_catalog_per_target() -> TestResult {
    let lock = parse_generator_lock(&lock_text(&"a".repeat(64)))?;
    let manifest = parse_release_manifest(&manifest_text(&"a".repeat(64)))?;
    verify_lock_against_manifest(&lock, &manifest)?;
    let tampered = parse_release_manifest(&manifest_text(&"b".repeat(64)))?;
    assert!(verify_lock_against_manifest(&lock, &tampered).is_err());
    let pin = format!("actions/checkout@{:040x}", 0);
    let planning = ir_with(vec![checkout_step(&pin)?, plan_step()])?;
    assert!(
        render_workflow_ir(&planning, WorkflowPolicy::VelnorRepositoryV1, None, &ctx()).is_err()
    );
    let no_download = ir_with(vec![checkout_step(&pin)?])?;
    assert!(
        render_workflow_ir(
            &no_download,
            WorkflowPolicy::VelnorRepositoryV1,
            None,
            &ctx()
        )
        .is_err()
    );
    Ok(())
}
