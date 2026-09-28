//! Fixed step-template and command-validation cases.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::{
    ASSET_SHA_ENV, ASSET_URL_ENV, RenderError, STAGED_BINARY_PREFIX, acquire_velnor_step,
    action_step, checkout_step, internal_step, join_argv_for_run, merge_step, plan_step,
    quote_run_arg, scan_for_private_subcommands, shell_step, validate_command_argv, validate_uses,
};

fn pin(name: &str) -> String {
    format!("{name}@{:040x}", 0)
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[test]
fn checkout_template_pins_action_without_credentials() -> Result<(), RenderError> {
    let step = checkout_step(&pin("actions/checkout"))?;
    assert_eq!(step.name, "Checkout");
    assert!(matches!(
        &step.kind,
        velnor_actions_contract::StepKind::Action { uses, with }
            if uses == &pin("actions/checkout")
                && with.get("persist-credentials").is_some_and(|v| v == "false")
    ));
    Ok(())
}

#[test]
fn uses_validation_rejects_moving_refs_and_forbidden_actions() {
    assert!(validate_uses(&pin("actions/checkout")).is_ok());
    assert!(validate_uses("actions/checkout@main").is_err());
    assert!(validate_uses("actions/checkout@v4").is_err());
    assert!(validate_uses("actions/checkout@ABCDEF").is_err());
    assert!(validate_uses("actions/checkout").is_err());
    assert!(validate_uses("just-a-name").is_err());
    assert!(validate_uses(&pin("actions/setup-node")).is_err());
    assert!(validate_uses(&pin("taiki-e/install-action")).is_err());
    assert!(checkout_step(&pin("actions/download-artifact")).is_err());
}

#[test]
fn shell_step_joins_fixed_argv_with_quoting() -> Result<(), RenderError> {
    let joined = join_argv_for_run(&argv(&[
        "mise",
        "exec",
        "--no-config",
        "rust@1.89.0",
        "--",
        "cargo",
        "test",
        "--package",
        "my crate",
    ]))?;
    assert_eq!(
        joined,
        "mise exec --no-config rust@1.89.0 -- cargo test --package 'my crate'"
    );
    let step = shell_step("Test", argv(&["cargo", "test"]), BTreeMap::new())?;
    assert!(matches!(
        &step.kind,
        velnor_actions_contract::StepKind::Shell { run, .. } if run.len() == 2
    ));
    Ok(())
}

#[test]
fn run_quoting_preserves_runner_expansion() {
    assert_eq!(
        quote_run_arg("$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0"),
        "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0"
    );
    assert_eq!(
        quote_run_arg("$RUNNER_TEMP/my dir/tool"),
        "$RUNNER_TEMP'/my dir/tool'"
    );
    assert_eq!(
        quote_run_arg("${{ github.run_id }}"),
        "${{ github.run_id }}"
    );
    assert_eq!(quote_run_arg("$HOME"), "$HOME");
    assert_eq!(quote_run_arg("plain"), "plain");
    assert_eq!(quote_run_arg("with space"), "'with space'");
    assert_eq!(quote_run_arg("it's"), "'it''\\'''s'");
}

#[test]
fn argv_validation_rejects_policy_violations() {
    assert!(validate_command_argv(&[]).is_err());
    assert!(validate_command_argv(&argv(&["cargo", "install", "x"])).is_err());
    assert!(validate_command_argv(&argv(&["/usr/bin/cargo", "test"])).is_err());
    assert!(validate_command_argv(&argv(&["/cargo", "test"])).is_err());
    assert!(validate_command_argv(&argv(&["echo", "$(evil)"])).is_err());
    assert!(validate_command_argv(&argv(&["echo", "`evil`"])).is_err());
    assert!(validate_command_argv(&argv(&["ok", ""])).is_err());
    assert!(validate_command_argv(&argv(&["bad\nline"])).is_err());
    assert!(validate_command_argv(&argv(&["velnor-actions", "__internal"])).is_err());
    assert!(validate_command_argv(&argv(&["mise", "exec", "--", "cargo", "test"])).is_ok());
}

#[test]
fn acquire_template_requires_digest_and_staging() {
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let mut env = BTreeMap::new();
    env.insert(ASSET_SHA_ENV.to_owned(), "a".repeat(64));
    env.insert(
        ASSET_URL_ENV.to_owned(),
        "https://example.invalid/v0.1.0/bin".to_owned(),
    );
    let good = acquire_velnor_step(argv(&["fetch", &staged]), env.clone());
    assert!(good.is_ok());
    assert_eq!(
        good.ok().map(|step| step.name),
        Some("Acquire Velnor".to_owned())
    );
    let mut bad_sha = env.clone();
    bad_sha.insert(ASSET_SHA_ENV.to_owned(), "zzz".to_owned());
    assert!(acquire_velnor_step(argv(&["fetch", &staged]), bad_sha).is_err());
    let mut bad_url = env.clone();
    bad_url.insert(
        ASSET_URL_ENV.to_owned(),
        "http://example.invalid/bin".to_owned(),
    );
    assert!(acquire_velnor_step(argv(&["fetch", &staged]), bad_url).is_err());
    assert!(acquire_velnor_step(argv(&["fetch", "/tmp/bin"]), env).is_err());
}

#[test]
fn internal_steps_accept_only_plan_and_merge() {
    assert!(internal_step("Plan", "plan-v1").is_ok());
    assert!(internal_step("Merge", "merge-v1").is_ok());
    assert!(internal_step("Sneaky", "__internal").is_err());
    assert!(internal_step("Sneaky", "run").is_err());
    assert!(internal_step("Sneaky", "plan").is_err());
    assert!(internal_step("", "plan-v1").is_err());
    assert_eq!(plan_step().name, "Plan");
    assert_eq!(merge_step().name, "Merge reports");
}

#[test]
fn private_subcommand_scan_finds_hidden_tokens() {
    assert!(scan_for_private_subcommands("run: velnor-actions plan").is_ok());
    assert!(scan_for_private_subcommands("velnor-actions __internal").is_err());
    assert!(scan_for_private_subcommands("velnor-actions run x").is_err());
    assert!(scan_for_private_subcommands("prefix __internal suffix").is_err());
    assert!(action_step("x", "velnor-actions __y", BTreeMap::new()).is_err());
}
