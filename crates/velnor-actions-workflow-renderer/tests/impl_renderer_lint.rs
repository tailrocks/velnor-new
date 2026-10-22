//! Always-on lint job cases: emitted from typed IR for both policies.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, ValidatorKind, VelnorSupportWorkflow,
    WorkflowIr, WorkflowPolicy, workflow::permissions::PermissionLevel,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, RenderError, ValidatorCommand,
    checkout_step, merge_step, render_workflow_ir, shell_step,
};

const VERSION: &str = "0.1.0";
const LABEL: &str = "ubuntu-26.04";
const LINT_ID: &str = "jactionlint";
const LINT_DISPLAY: &str = "Jactionlint";

fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

fn fixture_ctx() -> RenderContext {
    RenderContext {
        tool_pins: Vec::new(),
        generator_version: VERSION.to_owned(),
        report_helper_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        verified_cache_helpers: std::collections::BTreeMap::new(),
        require_verified_cache_helper: false,
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

fn lint_job() -> Result<Job, RenderError> {
    Ok(Job {
        services: Vec::new(),
        display_name: LINT_DISPLAY.to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checkout_step(&checkout_pin())?,
            shell_step(
                "Run jactionlint",
                vec![
                    "mise".to_owned(),
                    "--no-config".to_owned(),
                    "--no-env".to_owned(),
                    "--no-hooks".to_owned(),
                    "exec".to_owned(),
                    "jactionlint@2.3.0".to_owned(),
                    "shellcheck@0.11.0".to_owned(),
                    "--".to_owned(),
                    "jactionlint".to_owned(),
                    "--strict-exit".to_owned(),
                ],
                BTreeMap::new(),
            )?,
        ],
        cache_mode: None,
    })
}

fn fixture_ir() -> Result<WorkflowIr, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "plan".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Plan".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![checkout_step(&checkout_pin())?],
            cache_mode: None,
        },
    );
    jobs.insert(
        "required".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Required verdict".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned(), LINT_ID.to_owned()],
            condition: Some(velnor_actions_workflow_renderer::render::FINAL_CONDITION.to_owned()),
            permissions: Some(Permissions {
                contents: PermissionLevel::Read,
                actions: PermissionLevel::Read,
                packages: PermissionLevel::None,
                pull_requests: PermissionLevel::None,
                id_token: PermissionLevel::None,
            }),
            environment: None,
            steps: vec![merge_step()],
            cache_mode: None,
        },
    );
    jobs.insert(LINT_ID.to_owned(), lint_job()?);
    Ok(WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: [
                "opened",
                "edited",
                "synchronize",
                "reopened",
                "ready_for_review",
            ]
            .iter()
            .map(ToString::to_string)
            .collect(),
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
        jobs,
        cache_mode: None,
    })
}

fn velnor_support() -> VelnorSupportWorkflow {
    VelnorSupportWorkflow {
        validators: Vec::new(),
        candidate_validation: false,
    }
}

fn validator_commands() -> Vec<ValidatorCommand> {
    let mise = ["mise", "--no-config", "--no-env", "--no-hooks"];
    let deny = "cargo-deny@0.20.2";
    let machete = "http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]@0.9.2";
    vec![
        validator_command(
            ValidatorKind::CargoDeny,
            "Run cargo-deny",
            [&mise[..], &["exec", deny, "--", "cargo", "deny"][..]].concat(),
            [&mise[..], &["install", deny][..]].concat(),
        ),
        validator_command(
            ValidatorKind::CargoMachete,
            "Run cargo-machete",
            [&mise[..], &["exec", machete, "--", "cargo", "machete"][..]].concat(),
            [&mise[..], &["install", machete][..]].concat(),
        ),
    ]
}

fn validator_command(
    validator: ValidatorKind,
    name: &str,
    argv: Vec<&str>,
    prepare_argv: Vec<&str>,
) -> ValidatorCommand {
    ValidatorCommand {
        validator,
        name: name.to_owned(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        prepare_argv: prepare_argv.into_iter().map(str::to_owned).collect(),
    }
}

#[test]
fn consumer_emits_lint_from_typed_ir() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(text.contains("  jactionlint:"), "job:\n{text}");
    assert!(text.contains(LINT_DISPLAY), "display:\n{text}");
    assert!(
        text.contains("jactionlint@2.3.0 shellcheck@0.11.0 -- jactionlint --strict-exit"),
        "run:\n{text}"
    );
    assert!(text.contains("- jactionlint"), "final needs lint:\n{text}");
    Ok(())
}

#[test]
fn velnor_emits_lint_from_typed_ir() -> Result<(), RenderError> {
    let support = velnor_support();
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &fixture_ctx(),
    )?;
    assert!(text.contains("  jactionlint:"), "job:\n{text}");
    assert!(text.contains(LINT_DISPLAY), "display:\n{text}");
    assert!(text.contains("- jactionlint"), "final needs lint:\n{text}");
    Ok(())
}

#[path = "impl_renderer_lint_policy.rs"]
mod policy;
