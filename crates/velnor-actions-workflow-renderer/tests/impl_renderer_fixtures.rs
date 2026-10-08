//! Shared fixtures for the strict-emission test family.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, Trigger, ValidatorKind, WorkflowIr,
    WorkflowPolicy, workflow::permissions::PermissionLevel,
};
use velnor_actions_workflow_renderer::steps::{CompileDriver, mbx_steps_for_driver};
use velnor_actions_workflow_renderer::{
    ASSET_SHA_ENV, ASSET_URL_ENV, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, MiseSetup,
    RELEASE_COMMIT_ENV, RenderContext, RenderError, STAGED_BINARY_PREFIX, ValidatorCommand,
    acquire_velnor_step, checkout_step, plan_step, render_workflow_ir, render_workflow_ir_strict,
    shell_step,
};

pub(crate) const VERSION: &str = "0.1.0";
pub(crate) const LABEL: &str = "ubuntu-26.04";
pub(crate) const MISE_USES: &str = "jdx/mise-action@2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca";
pub(crate) const MISE_VERSION: &str = "2026.10.5";
pub(crate) const MISE_SHA256: &str =
    "8a223b5f8ca71100220a3e5bef259614c348e7b1d80e6b15c2a9c9aa3affe5e4";
pub(crate) const STAGED: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0";

pub(crate) const TEST_MBX_VERSION: &str = "1.21.1";
pub(crate) const TEST_RUST_TOOLCHAIN: &str = "1.98.1";

pub(crate) fn mbx_tool_env(rust_toolchain: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), rust_toolchain.to_owned()),
    ])
}

pub(crate) fn mbx_tool_steps(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
) -> Result<[Step; 3], RenderError> {
    mbx_steps_for_driver(
        uses,
        CompileDriver::Mbx,
        mbx_version,
        rust_toolchain,
        mbx_tool_env(rust_toolchain),
    )?
    .ok_or_else(|| RenderError::InvalidWorkflow("mbx_steps_missing".to_owned()))
}

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
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

pub(crate) fn acquire_fixture() -> Result<Step, RenderError> {
    let staged = format!("{STAGED_BINARY_PREFIX}{VERSION}");
    let script = format!(
        "mkdir -p $RUNNER_TEMP/velnor/bin && curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
    );
    acquire_velnor_step(
        vec!["sh".to_owned(), "-c".to_owned(), script],
        &BTreeMap::from([
            (ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (
                ASSET_URL_ENV.to_owned(),
                "https://example.invalid/releases/download/0.1.0/bin".to_owned(),
            ),
            (RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
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
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs,
            condition: None,
            permissions: (id == velnor_actions_workflow_renderer::render::FINAL_JOB_ID).then_some(
                Permissions {
                    contents: PermissionLevel::Read,
                    actions: PermissionLevel::Read,
                    pull_requests: PermissionLevel::None,
                    id_token: PermissionLevel::None,
                },
            ),
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
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    ))
}

/// One dummy shell command per repository validator needing one.
pub(crate) fn validator_commands() -> Vec<ValidatorCommand> {
    [
        (ValidatorKind::CargoDeny, "Run cargo-deny"),
        (ValidatorKind::CargoMachete, "Run cargo-machete"),
        (ValidatorKind::Zizmor, "Run zizmor"),
    ]
    .iter()
    .map(|(validator, name)| ValidatorCommand {
        validator: *validator,
        name: (*name).to_owned(),
        argv: vec!["true".to_owned()],
        prepare_argv: Vec::new(),
    })
    .collect()
}

/// Candidate context: validator commands plus build/qualify spec.
pub(crate) fn candidate_ctx() -> RenderContext {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
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
            "plan".to_owned(),
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
    // Unscrubbed base: the constructor owns the overlay.
    let step = shell_step(
        "Run task",
        vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
        env,
    )?;
    Ok(job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![checkout_step(&checkout_pin())?, step],
    ))
}

/// Shell fixture carrying the credential scrub overlay (D1 gate input).
///
/// The constructor applies the overlay; callers pass the bare base.
pub(crate) fn scrubbed_shell_step(name: &str, argv: Vec<String>) -> Result<Step, RenderError> {
    shell_step(name, argv, BTreeMap::new())
}

/// Minimal plan job with one caller-supplied shell step (hygiene input).
pub(crate) fn token_plan_job(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<(String, Job), RenderError> {
    Ok(job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(name, argv, env)?,
            plan_step(),
        ],
    ))
}

/// Assert a job set fails render with one error marker.
pub(crate) fn render_fails_with(jobs: Vec<(String, Job)>, want: &str) {
    assert!(
        render_workflow_ir(
            &fixture_ir(jobs),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )
        .is_err_and(|err| format!("{err:?}").contains(want)),
        "must fail with {want}"
    );
}
