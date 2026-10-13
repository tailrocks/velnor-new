//! Always-on lint job cases: emitted from typed IR for both policies.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, JobTimeout, Permissions, Trigger, ValidatorKind,
    VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy, workflow::permissions::PermissionLevel,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, RenderError, ValidatorCommand,
    checkout_step, merge_step, render_workflow_ir, shell_step,
};

use super::impl_renderer_fixtures::mise_argv;

const VERSION: &str = "0.1.0";
const LABEL: &str = "ubuntu-26.04";
const LINT_ID: &str = "actionlint";
const LINT_DISPLAY: &str = "Actionlint";

fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

fn fixture_ctx() -> RenderContext {
    RenderContext {
        generator_version: VERSION.to_owned(),
        report_helper_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
        scale_set_selector: None,
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
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

fn lint_job() -> Result<Job, RenderError> {
    Ok(Job {
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
                "Run actionlint",
                vec![
                    "mise".to_owned(),
                    "--no-config".to_owned(),
                    "--no-env".to_owned(),
                    "--no-hooks".to_owned(),
                    "exec".to_owned(),
                    "actionlint@1.7.12".to_owned(),
                    "shellcheck@0.11.0".to_owned(),
                    "--".to_owned(),
                    "actionlint".to_owned(),
                    "-color".to_owned(),
                ],
                BTreeMap::new(),
            )?,
        ],
    })
}

fn fixture_ir() -> Result<WorkflowIr, RenderError> {
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
            steps: vec![checkout_step(&checkout_pin())?],
        },
    );
    jobs.insert(
        "required".to_owned(),
        Job {
            display_name: "Required".to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned(), LINT_ID.to_owned()],
            condition: Some("always()".to_owned()),
            permissions: Some(Permissions {
                contents: PermissionLevel::Read,
                actions: PermissionLevel::Read,
                pull_requests: PermissionLevel::None,
                id_token: PermissionLevel::None,
            }),
            environment: None,
            steps: vec![merge_step()],
        },
    );
    jobs.insert(LINT_ID.to_owned(), lint_job()?);
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
    let zizmor = "zizmor@1.30.1";
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
        validator_command(
            ValidatorKind::Zizmor,
            "Run zizmor",
            scrubbed_zizmor_argv(zizmor),
            [&mise[..], &["install", zizmor][..]].concat(),
        ),
    ]
}

fn scrubbed_zizmor_argv(tool: &'static str) -> Vec<&'static str> {
    let mut argv = vec!["env"];
    for variable in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_TOKEN",
        "GH_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
    ] {
        argv.extend(["-u", variable]);
    }
    argv.extend([
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        tool,
        "--",
        "zizmor",
        "--no-online-audits",
    ]);
    argv
}

fn with_credential_unset(mut argv: Vec<String>) -> Vec<String> {
    let mut wrapped = vec!["env".to_owned()];
    for variable in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_TOKEN",
        "GH_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
    ] {
        wrapped.extend(["-u".to_owned(), variable.to_owned()]);
    }
    wrapped.append(&mut argv);
    wrapped
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
    assert!(text.contains("actionlint:"), "job:\n{text}");
    assert!(text.contains(LINT_DISPLAY), "display:\n{text}");
    assert!(
        text.contains("actionlint@1.7.12 shellcheck@0.11.0 -- actionlint -color"),
        "run:\n{text}"
    );
    assert!(text.contains("- actionlint"), "final needs lint:\n{text}");
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
    assert!(text.contains("actionlint:"), "job:\n{text}");
    assert!(text.contains(LINT_DISPLAY), "display:\n{text}");
    assert!(text.contains("- actionlint"), "final needs lint:\n{text}");
    Ok(())
}

#[test]
fn zizmor_support_unsets_empty_tokens_after_pinned_install() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = vec![ValidatorCommand {
        validator: ValidatorKind::Zizmor,
        name: "Run zizmor".to_owned(),
        prepare_argv: vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            "zizmor@1.30.1".to_owned(),
        ],
        argv: with_credential_unset(mise_argv(
            "zizmor@1.30.1",
            "zizmor",
            &[
                "--no-online-audits",
                "--config",
                ".zizmor.yml",
                ".github/workflows",
            ],
        )),
    }];
    let support = VelnorSupportWorkflow {
        validators: vec![ValidatorKind::Zizmor],
        candidate_validation: false,
    };
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    let zizmor_job = text
        .split_once("  zizmor:\n")
        .map(|(_, rest)| rest)
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_zizmor_job".to_owned()))?;
    let zizmor_job = zizmor_job
        .lines()
        .take_while(|line| line.starts_with("    ") || line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let install = zizmor_job
        .find("run: mise --no-config --no-env --no-hooks install zizmor@1.30.1")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_zizmor_install".to_owned()))?;
    let execute = zizmor_job
        .find("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_credential_unset".to_owned()))?;
    assert!(install < execute, "tool install must precede scrubbed exec");
    assert!(
        zizmor_job[execute..].contains("-u GH_TOKEN"),
        "GH_TOKEN must be unset"
    );
    assert!(zizmor_job[execute..].contains("exec zizmor@1.30.1 -- zizmor"));
    assert!(
        !zizmor_job.contains("GH_TOKEN: \"\""),
        "bootstrap must not inherit empty auth values"
    );
    Ok(())
}

#[test]
fn zizmor_preparation_keeps_job_env_and_exec_unsets_it() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    let zizmor = text
        .split_once("  zizmor:")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing zizmor job".to_owned()))?
        .1;
    let preparation = zizmor
        .split_once("- name: Run zizmor")
        .ok_or_else(|| RenderError::InvalidWorkflow("missing zizmor execution".to_owned()))?;
    assert!(
        preparation
            .0
            .contains("run: mise --no-config --no-env --no-hooks install zizmor@1.30.1")
    );
    assert!(!preparation.0.contains("env -u GH_TOKEN"));
    assert!(
        preparation
            .1
            .contains("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "validator command must remove inherited token environment:\n{zizmor}"
    );
    assert!(
        !zizmor.contains("GH_TOKEN: \"\""),
        "empty credentials break Mise bootstrap and must not be inherited:\n{zizmor}"
    );
    Ok(())
}

#[path = "impl_renderer_lint_policy.rs"]
mod policy;
