//! Catalog service containers render as the job's `services:` mapping.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, ServiceContainer, Step, StepKind, Trigger,
    WorkflowIr,
};

use super::workflow_to_yaml;
use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

fn job(services: Vec<ServiceContainer>) -> Job {
    Job {
        services,
        display_name: "Probe".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps: vec![Step {
            parallel: false,
            name: "Run probe".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["echo".to_owned(), "probe".to_owned()],
                env: BTreeMap::new(),
            },
        }],
        cache_mode: None,
    }
}

fn render(job: Job) -> String {
    let ctx = RenderContext {
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
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    };
    let jobs = BTreeMap::from([("probe".to_owned(), job)]);
    let shared = crate::lane_share::share_lanes(&jobs, &ctx).expect("lane sharing validates");
    let workflow = WorkflowIr {
        name: "Services probe".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
        cache_mode: None,
    };
    let rendered =
        workflow_to_yaml(&workflow, &shared, &ctx, &BTreeSet::new()).expect("workflow renders");
    crate::yaml::render_yaml(&rendered)
}

#[test]
fn postgres_renders_pinned_with_fixed_env_port_and_health_check() {
    let yaml = render(job(vec![ServiceContainer::postgres(18).expect("catalog")]));
    let services = yaml
        .split("    services:\n")
        .nth(1)
        .expect("services block");
    assert!(
        services.contains("      postgres:\n        image: postgres@sha256:"),
        "{yaml}"
    );
    assert!(services.contains("POSTGRES_PASSWORD: chainargos"), "{yaml}");
    assert!(services.contains("- 40000:5432"), "{yaml}");
    assert!(services.contains("--health-cmd"), "{yaml}");
    let before_steps =
        yaml.find("    services:").expect("services") < yaml.find("    steps:").expect("steps");
    assert!(before_steps, "services precede steps: {yaml}");
}

#[test]
fn a_job_without_services_renders_none() {
    assert!(!render(job(Vec::new())).contains("services:"));
}
