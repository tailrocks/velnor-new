//! Matrix strategy emission: shape, caps, stripping, fail-closed.
use std::collections::BTreeMap;
use velnor_actions_contract::{Concurrency, Job, Permissions, Trigger, WorkflowIr, WorkflowPolicy};
use velnor_actions_workflow_renderer::render::{
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, RenderError, checkout_step, plan_step,
    render_workflow_ir, shell_step,
};

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
    }
}

fn fixture_ir(task: Job) -> Result<WorkflowIr, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "plan".to_owned(),
        Job {
            display_name: "Plan".to_owned(),
            runs_on: LABEL.to_owned(),
            needs: Vec::new(),
            condition: None,
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
        },
        permissions: Permissions {
            contents: "read".to_owned(),
            actions: "read".to_owned(),
        },
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    })
}

fn task_job(env: BTreeMap<String, String>, needs: Vec<String>) -> Result<Job, RenderError> {
    let argv = ["sh", "-c", "echo hi"].map(str::to_owned).to_vec();
    Ok(Job {
        display_name: "Task".to_owned(),
        runs_on: LABEL.to_owned(),
        needs,
        condition: None,
        steps: vec![
            checkout_step(&checkout_pin())?,
            shell_step("Run task", argv, env)?,
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
    let text = render(task_job(marker_env("2"), vec!["plan".to_owned()])?)?;
    for line in [
        "    strategy:",
        "      fail-fast: false",
        "      max-parallel: 2",
        "      matrix: ${{ fromJSON(needs.plan.outputs.matrix) }}",
        "    outputs:",
        "      matrix: ${{ steps.plan.outputs.matrix }}",
        "        id: plan",
    ] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    assert!(!text.contains("VELNOR_MATRIX_"), "stripped:\n{text}");
    Ok(())
}

#[test]
fn max_parallel_honored() -> Result<(), RenderError> {
    let text = render(task_job(marker_env("7"), vec!["plan".to_owned()])?)?;
    assert!(text.contains("max-parallel: 7"), "cap:\n{text}");
    Ok(())
}

#[test]
fn static_task_renders_no_strategy() -> Result<(), RenderError> {
    let text = render(task_job(BTreeMap::new(), vec!["plan".to_owned()])?)?;
    for absent in ["strategy:", "outputs:", "id: plan", "fromJSON"] {
        assert!(!text.contains(absent), "static hit {absent}:\n{text}");
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
        assert!(render(task_job(env, needs)?).is_err_and(|err| format!("{err:?}").contains(want)));
    }
    Ok(())
}
