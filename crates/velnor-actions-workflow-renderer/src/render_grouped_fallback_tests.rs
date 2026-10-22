use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepId, StepKind, StepRole, Trigger,
    WorkflowIr,
};

use super::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, PLAN_JOB_ID, RenderContext, render_merged_with_routing,
};
use crate::{RenderError, workflow_size::MAX_WORKFLOW_BYTES};

const OBLIGATION_COUNT: usize = 340;
const RUN_BYTES: usize = 2_000;

fn context() -> RenderContext {
    RenderContext {
        tool_pins: Vec::new(),
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        verified_cache_helpers: BTreeMap::new(),
        require_verified_cache_helper: false,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: "actions/checkout@0000000000000000000000000000000000000000".to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn workflow() -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned(), "synchronize".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::new(),
        cache_mode: None,
    }
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

fn grouped_jobs() -> BTreeMap<String, Job> {
    let plan = Job {
        services: Vec::new(),
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: "Create plan".to_owned(),
            id: Some(StepId::Plan),
            role: Some(StepRole::PlanProducer),
            condition: None,
            parallel: false,
            kind: StepKind::Internal {
                operation: "plan-v1".to_owned(),
                env: BTreeMap::new(),
            },
        }],
        cache_mode: None,
    };
    let payload = "x".repeat(RUN_BYTES);
    let mut steps = Vec::with_capacity(OBLIGATION_COUNT);
    for index in 0..OBLIGATION_COUNT {
        steps.push(Step {
            name: format!("Run configuration {index:03}"),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["printf".to_owned(), "%s".to_owned(), payload.clone()],
                env: BTreeMap::new(),
            },
        });
    }
    BTreeMap::from([
        (PLAN_JOB_ID.to_owned(), plan),
        ("rust-demo".to_owned(), job(steps)),
    ])
}

#[test]
fn large_grouped_job_uses_yaml_sharing_without_obligation_actions() -> Result<(), RenderError> {
    let jobs = grouped_jobs();
    let rendered = render_merged_with_routing(
        &workflow(),
        &jobs,
        &context(),
        &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )?;

    assert!(rendered.yaml.len() <= MAX_WORKFLOW_BYTES);
    assert!(rendered.yaml.contains("run: &r1"));
    assert!(rendered.yaml.contains("run: *r1"));
    assert!(rendered.yaml.contains("Run configuration 339"));
    assert!(!rendered.yaml.contains("covered_tasks"));
    assert!(
        rendered
            .shared
            .iter()
            .all(|file| !file.path.contains("crate-obligation-"))
    );
    Ok(())
}

#[test]
fn large_grouped_job_rendering_is_byte_stable() -> Result<(), RenderError> {
    let jobs = grouped_jobs();
    let first = render_merged_with_routing(
        &workflow(),
        &jobs,
        &context(),
        &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )?;
    let second = render_merged_with_routing(
        &workflow(),
        &jobs,
        &context(),
        &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )?;
    assert_eq!(first, second);
    Ok(())
}
