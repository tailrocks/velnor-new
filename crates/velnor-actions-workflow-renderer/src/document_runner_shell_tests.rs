use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CacheMode, Concurrency, Job, JobTimeout, Permissions, SCALE_SET_NAME, ScaleSetSelector, Step,
    StepKind, Trigger, VELNOR_LABEL, WorkflowIr,
};

use super::workflow_to_yaml;
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
        cache_mode: None,
    }
}

fn context() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
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
        cache_mode: None,
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
    let yaml = crate::yaml::render_yaml(&rendered);
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

#[test]
fn credential_scrub_is_factored_to_workflow_env() {
    let mut probe = job("ubuntu-26.04");
    probe.steps[0].kind = StepKind::Shell {
        run: vec!["echo".to_owned(), "probe".to_owned()],
        env: BTreeMap::from([("PROBE_VALUE".to_owned(), "visible".to_owned())]),
    };
    let jobs = BTreeMap::from([("probe".to_owned(), probe)]);
    let ctx = context();
    let shared = crate::lane_share::share_lanes(&jobs, &ctx).expect("lane sharing validates");
    let rendered = workflow_to_yaml(&workflow(jobs), &shared, &ctx, &BTreeSet::new())
        .expect("workflow renders");
    let yaml = crate::yaml::render_yaml(&rendered);
    let workflow_env = yaml
        .split_once("env:\n")
        .and_then(|(_, rest)| rest.split_once("\nconcurrency:"))
        .map(|(env, _)| env)
        .expect("workflow environment");
    let probe = yaml.split("  probe:\n").nth(1).expect("probe job");

    assert_eq!(yaml.matches("GH_TOKEN: \"\"").count(), 1);
    assert!(yaml.contains("env:\n  ACTIONS_ID_TOKEN_REQUEST_TOKEN: \"\""));
    assert!(workflow_env.contains("MISE_NO_CONFIG: \"1\""));
    assert!(workflow_env.contains("MISE_EXEC_AUTO_INSTALL: \"false\""));
    assert!(!probe.contains("GH_TOKEN: \"\""));
    assert!(!probe.contains("MISE_NO_CONFIG: \"1\""));
    assert!(probe.contains("PROBE_VALUE: visible"));
}

#[test]
fn mise_quiet_opt_out_removes_exactly_the_quiet_line() {
    let jobs = BTreeMap::from([("probe".to_owned(), job("ubuntu-26.04"))]);
    let quiet_ctx = context();
    let loud_ctx = RenderContext {
        mise_quiet: false,
        ..context()
    };
    let quiet_shared =
        crate::lane_share::share_lanes(&jobs, &quiet_ctx).expect("lane sharing validates");
    let quiet = crate::yaml::render_yaml(
        &workflow_to_yaml(
            &workflow(jobs.clone()),
            &quiet_shared,
            &quiet_ctx,
            &BTreeSet::new(),
        )
        .expect("quiet workflow renders"),
    );
    let loud_shared =
        crate::lane_share::share_lanes(&jobs, &loud_ctx).expect("lane sharing validates");
    let loud = crate::yaml::render_yaml(
        &workflow_to_yaml(&workflow(jobs), &loud_shared, &loud_ctx, &BTreeSet::new())
            .expect("loud workflow renders"),
    );

    let quiet_line = "  MISE_QUIET: \"1\"\n";
    assert_eq!(quiet.matches(quiet_line).count(), 1, "{quiet}");
    assert_eq!(loud.matches(quiet_line).count(), 0, "{loud}");
    let mut without_quiet = quiet.clone();
    let start = without_quiet.find(quiet_line).expect("quiet line");
    without_quiet.replace_range(start..start + quiet_line.len(), "");
    assert_eq!(
        loud, without_quiet,
        "opting out must change no byte except the MISE_QUIET line"
    );
}

fn field<'a>(value: &'a crate::yaml::Yaml, key: &str) -> Option<&'a crate::yaml::Yaml> {
    let crate::yaml::Yaml::Map(entries) = value else {
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
    if let Some(job_env) = field(task, "env") {
        assert_eq!(
            field(job_env, "RUSTDOCFLAGS"),
            None,
            "task-specific rustdoc flags must not enter the job environment"
        );
    }
    let crate::yaml::Yaml::Seq(steps) = field(task, "steps").expect("job steps") else {
        panic!("steps must be a sequence");
    };
    let doc = steps
        .iter()
        .find(|step| field(step, "name") == Some(&crate::yaml::Yaml::str("Documentation")))
        .expect("documentation step");
    let doc_env = field(doc, "env").expect("documentation step environment");
    assert_eq!(
        field(doc_env, "RUSTDOCFLAGS"),
        Some(&crate::yaml::Yaml::str("-D warnings")),
        "the documentation step keeps its typed flags"
    );
    let tests = steps
        .iter()
        .find(|step| field(step, "name") == Some(&crate::yaml::Yaml::str("Tests")))
        .expect("test step");
    if let Some(test_env) = field(tests, "env") {
        assert_eq!(
            field(test_env, "RUSTDOCFLAGS"),
            None,
            "sibling tasks must not inherit documentation flags"
        );
    }
}

#[test]
fn cache_mode_renders_at_workflow_and_job_level_when_configured_and_omits_when_none() {
    let mut read_job = job("ubuntu-26.04");
    read_job.cache_mode = Some(CacheMode::Read);
    let default_job = job("ubuntu-26.04");
    let jobs = BTreeMap::from([
        ("read-only".to_owned(), read_job),
        ("default".to_owned(), default_job),
    ]);
    let ctx = context();
    let shared = crate::lane_share::share_lanes(&jobs, &ctx).expect("lane sharing validates");

    let omit_wf = workflow(jobs.clone());
    let omit_rendered =
        workflow_to_yaml(&omit_wf, &shared, &ctx, &BTreeSet::new()).expect("workflow renders");
    assert_eq!(
        field(&omit_rendered, "cache-mode"),
        None,
        "workflow cache-mode must be omitted when None"
    );

    let mut expr_wf = workflow(jobs);
    expr_wf.cache_mode = Some(CacheMode::Expression(
        "github.event_name == 'pull_request' && 'read' || 'write'".to_owned(),
    ));
    let expr_rendered =
        workflow_to_yaml(&expr_wf, &shared, &ctx, &BTreeSet::new()).expect("workflow renders");
    assert_eq!(
        field(&expr_rendered, "cache-mode"),
        Some(&crate::yaml::Yaml::str(
            "github.event_name == 'pull_request' && 'read' || 'write'"
        ))
    );
    let rendered_jobs = field(&expr_rendered, "jobs").expect("jobs map");
    let read_only = field(rendered_jobs, "read-only").expect("read-only job");
    let default_rendered = field(rendered_jobs, "default").expect("default job");
    assert_eq!(
        field(read_only, "cache-mode"),
        Some(&crate::yaml::Yaml::str("read"))
    );
    assert_eq!(
        field(default_rendered, "cache-mode"),
        None,
        "job cache-mode must be omitted when None"
    );
}
