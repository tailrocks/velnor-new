//! Matrix strategy emission: shape, caps, stripping, fail-closed.
use std::collections::BTreeMap;
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr,
};
use velnor_actions_workflow_jobs::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};
use velnor_actions_workflow_renderer::render::{
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
};
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{RenderError, checkout_step, plan_step, shell_step};

const VERSION: &str = "0.1.0";
const LABEL: &str = "ubuntu-26.04";

fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

fn fixture_ctx() -> RenderContext {
    RenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

fn fixture_ir(task: Job) -> Result<WorkflowIr, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "plan".to_owned(),
        Job {
            display_name: "Plan".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?, plan_step()],
        },
    );
    jobs.insert("velnor-task".to_owned(), task);
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

fn task_job(env: &BTreeMap<String, String>, needs: Vec<String>) -> Result<Job, RenderError> {
    let argv = ["sh", "-c", "echo hi"].map(str::to_owned).to_vec();
    Ok(Job {
        display_name: "Task".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs,
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checkout_step(&checkout_pin())?,
            shell_step("Run task", argv, env.clone())?,
        ],
    })
}

fn marker_env(max: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("VELNOR_TASK_ID".into(), "${{ matrix.task_id }}".into()),
        ("VELNOR_TASK_RUN".into(), "${{ matrix.run }}".into()),
        (MATRIX_NEEDS_JOB_ENV.into(), "plan".into()),
        (MATRIX_OUTPUT_ENV.into(), "matrix".into()),
        (MATRIX_MAX_PARALLEL_ENV.into(), max.into()),
    ])
}

fn render(task: Job) -> Result<String, RenderError> {
    let ir = fixture_ir(task)?;
    render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())
}

#[test]
fn strategy_shape_exact_and_marker_stripped() -> Result<(), RenderError> {
    let text = render(task_job(&marker_env("2"), vec!["plan".to_owned()])?)?;
    for line in [
        "    strategy:",
        "      fail-fast: false",
        "      max-parallel: 2",
        "      matrix: ${{ fromJSON(needs.plan.outputs.matrix) }}",
        "    outputs:",
        "      matrix: ${{ steps.plan.outputs.matrix }}",
        "VELNOR_PLAN_MATRIX_OUTPUT_MODE: dynamic_matrix",
        "        id: plan",
    ] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    assert!(!text.contains("VELNOR_MATRIX_"), "stripped:\n{text}");
    Ok(())
}

#[test]
fn max_parallel_honored() -> Result<(), RenderError> {
    let text = render(task_job(&marker_env("7"), vec!["plan".to_owned()])?)?;
    assert!(text.contains("max-parallel: 7"), "cap:\n{text}");
    Ok(())
}

#[test]
fn static_task_renders_no_strategy() -> Result<(), RenderError> {
    let text = render(task_job(&BTreeMap::new(), vec!["plan".to_owned()])?)?;
    assert!(
        !text.contains("VELNOR_PLAN_MATRIX_OUTPUT_MODE"),
        "static mode:\n{text}"
    );
    // The plan job legitimately publishes `covered_tasks`; only the
    // static task job must stay free of matrix machinery.
    let start = text.find("velnor-task:").expect("task job renders");
    let task = &text[start..];
    for absent in ["strategy:", "outputs:", "id: plan", "fromJSON"] {
        assert!(!task.contains(absent), "static hit {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn matrix_misuse_fails_closed() -> Result<(), RenderError> {
    let partial = BTreeMap::from([(MATRIX_NEEDS_JOB_ENV.to_owned(), "plan".to_owned())]);
    for (env, needs, want) in [
        (partial, vec!["plan".to_owned()], "matrix_marker_partial"),
        (
            marker_env("0"),
            vec!["plan".to_owned()],
            "matrix_bad_max_parallel",
        ),
        (marker_env("2"), Vec::new(), "matrix_without_producer_need"),
    ] {
        assert!(render(task_job(&env, needs)?).is_err_and(|err| format!("{err:?}").contains(want)));
    }
    Ok(())
}

/// IR with the plan job plus one crate job (no task job).
fn fixture_ir_with(id: &str, job: Job) -> Result<WorkflowIr, RenderError> {
    let mut ir = fixture_ir(task_job(&BTreeMap::new(), vec!["plan".to_owned()])?)?;
    ir.jobs.remove("velnor-task");
    ir.jobs.insert(id.to_owned(), job);
    Ok(ir)
}

/// Crate job with one trio-carrying shell step.
fn capped_job(env: &BTreeMap<String, String>, needs: Vec<String>) -> Result<Job, RenderError> {
    let argv = ["sh", "-c", "echo hi"].map(str::to_owned).to_vec();
    Ok(Job {
        display_name: "Rust / stacks-a".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs,
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checkout_step(&checkout_pin())?,
            shell_step("Validate", argv, env.clone())?,
        ],
    })
}

fn cap_env(max: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (MATRIX_NEEDS_JOB_ENV.into(), "plan".into()),
        (MATRIX_OUTPUT_ENV.into(), "covered_tasks".into()),
        (MATRIX_MAX_PARALLEL_ENV.into(), max.into()),
    ])
}

fn render_capped(id: &str, job: Job) -> Result<String, RenderError> {
    let ir = fixture_ir_with(id, job)?;
    render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())
}

/// YAML slice of one rendered job.
fn job_window<'a>(yaml: &'a str, id: &str) -> Result<&'a str, RenderError> {
    let header = format!("  {id}:");
    let from = yaml
        .find(&header)
        .ok_or_else(|| RenderError::InvalidWorkflow("job header".to_owned()))?;
    let mut end = yaml.len();
    let mut at = from + header.len();
    for line in yaml[at..].split_inclusive('\n') {
        let trimmed = line.trim_end().strip_prefix("  ").unwrap_or("");
        if !trimmed.starts_with(' ')
            && trimmed.len() > 1
            && trimmed.ends_with(':')
            && !trimmed.contains(' ')
        {
            end = at;
            break;
        }
        at += line.len();
    }
    Ok(&yaml[from..end])
}

#[test]
fn capped_crate_job_renders_cap_without_matrix() -> Result<(), RenderError> {
    let id = "rust-stacks-a";
    let text = render_capped(id, capped_job(&cap_env("3"), vec!["plan".to_owned()])?)?;
    let window = job_window(&text, id)?;
    assert!(window.contains("max-parallel: 3"), "cap:\n{window}");
    for absent in ["fromJSON", "matrix:", "VELNOR_MATRIX_"] {
        assert!(!window.contains(absent), "capped hit {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn uncapped_crate_job_stays_static() -> Result<(), RenderError> {
    let id = "rust-demo";
    let text = render_capped(id, capped_job(&BTreeMap::new(), vec!["plan".to_owned()])?)?;
    let window = job_window(&text, id)?;
    for absent in ["strategy:", "fromJSON", "VELNOR_MATRIX_"] {
        assert!(!window.contains(absent), "static hit {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn crate_cap_misuse_fails_closed() -> Result<(), RenderError> {
    let id = "rust-stacks-a";
    let partial = BTreeMap::from([(MATRIX_MAX_PARALLEL_ENV.to_owned(), "2".to_owned())]);
    for (env, needs, want) in [
        (partial, vec!["plan".to_owned()], "matrix_marker_partial"),
        (
            cap_env("0"),
            vec!["plan".to_owned()],
            "matrix_bad_max_parallel",
        ),
        (
            cap_env("nope"),
            vec!["plan".to_owned()],
            "matrix_bad_max_parallel",
        ),
        (cap_env("2"), Vec::new(), "matrix_without_producer_need"),
    ] {
        assert!(
            render_capped(id, capped_job(&env, needs)?)
                .is_err_and(|err| format!("{err:?}").contains(want)),
            "want {want}"
        );
    }
    Ok(())
}
