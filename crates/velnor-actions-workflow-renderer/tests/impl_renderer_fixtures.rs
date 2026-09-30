//! Shared fixtures for the strict-emission test family.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, Job, Permissions, Step, Trigger, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_workflow_renderer::{
    ASSET_SHA_ENV, ASSET_URL_ENV, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, MiseSetup, RenderContext,
    RenderError, STAGED_BINARY_PREFIX, acquire_velnor_step, checkout_step, plan_step,
    render_workflow_ir_strict, shell_step,
};

pub(crate) const VERSION: &str = "0.1.0";
pub(crate) const LABEL: &str = "ubuntu-26.04";
pub(crate) const MISE_USES: &str = "jdx/mise-action@c2a87611a18de5b3828c5652fe268e992400cb5c";
pub(crate) const MISE_VERSION: &str = "2026.9.16";
pub(crate) const MISE_SHA256: &str =
    "b6f8757201f6a2ee799f45f3f52ef7ca0b4071523637dc3b0b24264dd3333518";
pub(crate) const STAGED: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0";

pub(crate) fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

pub(crate) fn mise() -> MiseSetup {
    MiseSetup {
        uses: MISE_USES.to_owned(),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_SHA256.to_owned(),
    }
}

pub(crate) fn fixture_ctx() -> RenderContext {
    RenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        staged_binary: STAGED.to_owned(),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        policy_commands: Vec::new(),
        candidate: None,
        preseed: false,
    }
}

pub(crate) fn acquire_fixture() -> Result<Step, RenderError> {
    let staged = format!("{STAGED_BINARY_PREFIX}{VERSION}");
    let script = format!(
        "mkdir -p $RUNNER_TEMP/velnor/bin && curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
    );
    acquire_velnor_step(
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::from([
            (ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (
                ASSET_URL_ENV.to_owned(),
                "https://example.invalid/releases/download/0.1.0/bin".to_owned(),
            ),
        ]),
    )
}

pub(crate) fn mise_argv(tool: &str, program: &str, extra: &[&str]) -> Vec<String> {
    let mut argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        tool.to_owned(),
        "--".to_owned(),
        program.to_owned(),
    ];
    argv.extend(extra.iter().map(ToString::to_string));
    argv
}

pub(crate) fn job(id: &str, display: &str, needs: Vec<String>, steps: Vec<Step>) -> (String, Job) {
    (
        id.to_owned(),
        Job {
            display_name: display.to_owned(),
            runs_on: LABEL.to_owned(),
            needs,
            condition: None,
            permissions: None,
            environment: None,
            steps,
        },
    )
}

pub(crate) fn fixture_ir(jobs: Vec<(String, Job)>) -> WorkflowIr {
    WorkflowIr {
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
        jobs: jobs.into_iter().collect(),
    }
}

pub(crate) fn strict(ir: &WorkflowIr, ctx: &RenderContext) -> Result<String, RenderError> {
    render_workflow_ir_strict(ir, WorkflowPolicy::ConsumerV1, None, ctx, &mise())
}

/// Step display names in render order for one job section.
pub(crate) fn step_names(text: &str, job_id: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            inside = line.trim() == format!("{job_id}:").as_str();
        } else if inside && let Some(name) = line.trim_start().strip_prefix("- name: ") {
            names.push(name.trim_matches('"').to_owned());
        }
    }
    names
}

/// Bounded byte window after an offset (ASCII-safe render text).
pub(crate) fn snip(text: &str, at: usize, len: usize) -> &str {
    let end = at.saturating_add(len).min(text.len());
    text.get(at..end).unwrap_or_default()
}

/// Smallest plan job: checkout plus the `plan-v1` anchor.
pub(crate) fn minimal_plan_job() -> Result<(String, Job), RenderError> {
    Ok(job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    ))
}

/// Candidate context: pinned-tools policy command plus build/qualify spec.
pub(crate) fn candidate_ctx() -> RenderContext {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![velnor_actions_workflow_renderer::PolicyCommand {
        name: "Verify pinned tools".to_owned(),
        argv: vec!["true".to_owned()],
    }];
    ctx.candidate = Some(velnor_actions_workflow_renderer::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    ctx
}

/// Task job wired for matrix fan-out plus the matrix report upload.
pub(crate) fn matrix_task_job() -> Result<(String, velnor_actions_contract::Job), RenderError> {
    let env = BTreeMap::from([
        (
            "VELNOR_TASK_ID".to_owned(),
            "${{ matrix.task_id }}".to_owned(),
        ),
        ("VELNOR_TASK_RUN".to_owned(), "${{ matrix.run }}".to_owned()),
        (
            velnor_actions_workflow_renderer::MATRIX_NEEDS_JOB_ENV.to_owned(),
            "velnor-plan".to_owned(),
        ),
        (
            velnor_actions_workflow_renderer::MATRIX_OUTPUT_ENV.to_owned(),
            "matrix".to_owned(),
        ),
        (
            velnor_actions_workflow_renderer::MATRIX_MAX_PARALLEL_ENV.to_owned(),
            "2".to_owned(),
        ),
    ]);
    let step = shell_step(
        "Run task",
        vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
        env,
    )?;
    Ok(job(
        "velnor-task",
        "Velnor Task",
        vec!["velnor-plan".to_owned()],
        vec![checkout_step(&checkout_pin())?, step],
    ))
}
