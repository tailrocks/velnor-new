use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr,
};

use super::super::workflow_to_yaml;
use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

fn runner_token() -> String {
    ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("valid scale set")
    .token()
}

fn job(runs_on: &str) -> Job {
    Job {
        display_name: "Probe".to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps: vec![Step {
            name: "Run probe".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["echo".to_owned(), "probe".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    }
}

fn context() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    }
}

fn workflow(jobs: BTreeMap<String, Job>) -> WorkflowIr {
    WorkflowIr {
        name: "Paired probe".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    }
}

#[test]
fn typed_scale_set_jobs_declare_bash_while_hosted_jobs_keep_default() {
    let jobs = BTreeMap::from([
        ("hosted".to_owned(), job("ubuntu-26.04")),
        ("scale".to_owned(), job(&runner_token())),
    ]);
    let ctx = context();
    let shared = crate::lane_share::share_lanes(&jobs, &ctx).expect("lane sharing validates");
    let rendered = workflow_to_yaml(&workflow(jobs), &shared, &ctx, &BTreeSet::new())
        .expect("workflow renders");
    let yaml = velnor_actions_workflow_tree::yaml::render_yaml(&rendered);
    assert!(
        yaml.contains("- name: Run probe\n        run: echo probe"),
        "generated shell steps must put name first for shell scanning: {yaml}"
    );
    let hosted = yaml.split("  hosted:\n").nth(1).expect("hosted job");
    let hosted = hosted.split("  scale:\n").next().expect("hosted boundary");
    let scale = yaml.split("  scale:\n").nth(1).expect("scale job");
    assert!(!hosted.contains("defaults:"), "{hosted}");
    assert!(
        scale.contains("defaults:\n      run:\n        shell: bash -e {0}"),
        "{scale}"
    );
}

fn field<'a>(
    value: &'a velnor_actions_workflow_tree::yaml::Yaml,
    key: &str,
) -> Option<&'a velnor_actions_workflow_tree::yaml::Yaml> {
    let velnor_actions_workflow_tree::yaml::Yaml::Map(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(entry_key, _)| entry_key == key)
        .map(|(_, entry_value)| entry_value)
}

#[test]
fn rustdocflags_remain_scoped_to_the_documentation_step() {
    let doc_step = Step {
        name: "Documentation".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["echo".to_owned(), "doc".to_owned()],
            env: BTreeMap::from([("RUSTDOCFLAGS".to_owned(), "-D warnings".to_owned())]),
        },
    };
    let test_step = Step {
        name: "Tests".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["echo".to_owned(), "test".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mut task_job = job("ubuntu-26.04");
    task_job.steps = vec![doc_step, test_step];
    let jobs = BTreeMap::from([("task".to_owned(), task_job)]);
    let ctx = context();
    let shared = crate::lane_share::share_lanes(&jobs, &ctx).expect("lane sharing validates");
    let rendered = workflow_to_yaml(&workflow(jobs), &shared, &ctx, &BTreeSet::new())
        .expect("workflow renders");
    let jobs = field(&rendered, "jobs").expect("jobs map");
    let task = field(jobs, "task").expect("task job");
    let job_env = field(task, "env").expect("job environment");
    assert_eq!(
        field(job_env, "RUSTDOCFLAGS"),
        None,
        "task-specific rustdoc flags must not enter the job environment"
    );
    let velnor_actions_workflow_tree::yaml::Yaml::Seq(steps) =
        field(task, "steps").expect("job steps")
    else {
        panic!("steps must be a sequence");
    };
    let doc = steps
        .iter()
        .find(|step| {
            field(step, "name")
                == Some(&velnor_actions_workflow_tree::yaml::Yaml::str(
                    "Documentation",
                ))
        })
        .expect("documentation step");
    let doc_env = field(doc, "env").expect("documentation step environment");
    assert_eq!(
        field(doc_env, "RUSTDOCFLAGS"),
        Some(&velnor_actions_workflow_tree::yaml::Yaml::str(
            "-D warnings"
        )),
        "the documentation step keeps its typed flags"
    );
    let tests = steps
        .iter()
        .find(|step| {
            field(step, "name") == Some(&velnor_actions_workflow_tree::yaml::Yaml::str("Tests"))
        })
        .expect("test step");
    if let Some(test_env) = field(tests, "env") {
        assert_eq!(
            field(test_env, "RUSTDOCFLAGS"),
            None,
            "sibling tasks must not inherit documentation flags"
        );
    }
}
