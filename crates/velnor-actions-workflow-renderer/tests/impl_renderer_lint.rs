//! Always-on lint job cases: emitted from typed IR for both policies.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    Concurrency, GeneratorValidation, Job, JobTimeout, Permissions, Trigger, ValidatorKind,
    VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_workflow_renderer::{
    ALINT_USES, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, RenderError,
    ValidatorCommand, checkout_step, merge_step, render_workflow_ir, shell_step,
};

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
        runs_on: LABEL.to_owned(),
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: checkout_pin(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        plan_consumer_env: std::collections::BTreeMap::new(),
    }
}

fn lint_job() -> Result<Job, RenderError> {
    Ok(Job {
        display_name: LINT_DISPLAY.to_owned(),
        runs_on: LABEL.to_owned(),
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
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec!["plan".to_owned(), LINT_ID.to_owned()],
            condition: Some("always()".to_owned()),
            permissions: None,
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
    let machete = "ubi:bnjbvr/cargo-machete@0.9.2";
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
            [
                &mise[..],
                &["exec", zizmor, "--", "zizmor", "--no-online-audits"][..],
            ]
            .concat(),
            [&mise[..], &["install", zizmor][..]].concat(),
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
fn bad_lint_display_rejected_on_both_policies() -> Result<(), RenderError> {
    let mut ir = fixture_ir()?;
    if let Some(lint) = ir.jobs.get_mut(LINT_ID) {
        lint.display_name = "Wrong".to_owned();
    }
    let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &fixture_ctx())
        .err()
        .ok_or_else(|| RenderError::InvalidWorkflow("consumer accepted".to_owned()))?;
    assert!(err.to_string().contains("bad_lint_name"), "got {err}");
    let support = velnor_support();
    let err = render_workflow_ir(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &fixture_ctx(),
    )
    .err()
    .ok_or_else(|| RenderError::InvalidWorkflow("velnor accepted".to_owned()))?;
    assert!(err.to_string().contains("bad_lint_name"), "got {err}");
    Ok(())
}

#[test]
fn velnor_policy_emits_full_sha_alint_pin() -> Result<(), RenderError> {
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
    assert_eq!(
        ALINT_USES,
        "asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"
    );
    assert!(text.contains("  alint:"), "alint job missing:\n{text}");
    assert!(
        text.contains("uses: asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"),
        "full-SHA pin missing:\n{text}"
    );
    assert!(
        !text.contains("asamarts/alint@v"),
        "tag ref emitted:\n{text}"
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
        zizmor.contains("GH_TOKEN: \"\""),
        "scrubbed environment must be rendered:\n{zizmor}"
    );
    Ok(())
}
