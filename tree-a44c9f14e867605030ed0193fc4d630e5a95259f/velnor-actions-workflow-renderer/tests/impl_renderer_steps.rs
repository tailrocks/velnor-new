//! Fixed step-template and command-validation cases.
//!
//! Run-string quoting and join cases live in
//! `impl_renderer_steps_quote`.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::{
    ASSET_SHA_ENV, ASSET_URL_ENV, RELEASE_COMMIT_ENV, RenderError, STAGED_BINARY_PREFIX,
    acquire_velnor_step, action_step, checkout_step, internal_step, merge_step, plan_step,
    scan_for_private_subcommands, shell_step, validate_command_argv, validate_env, validate_uses,
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
        velnor_actions_contract_workflow::StepKind::Action { uses, with, .. }
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
    assert!(validate_uses("asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb").is_ok());
    assert!(validate_uses("asamarts/alint@v0.16.1").is_err());
    assert!(validate_uses("actions/checkout@ABCDEF").is_err());
    assert!(validate_uses("actions/checkout").is_err());
    assert!(validate_uses("just-a-name").is_err());
    assert!(validate_uses(&pin("actions/setup-node")).is_err());
    assert!(validate_uses(&pin("taiki-e/install-action")).is_err());
    assert!(checkout_step(&pin("actions/download-artifact")).is_err());
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
fn argv_accepts_expressions_for_gate_diagnosis() {
    // `${{ }}` stays constructible in argv: the release gates diagnose
    // secret/input handles with specific tokens after this check.
    for arg in ["${{ secrets.x }}", "${{secrets.x}}", "${{ github.sha }}"] {
        assert!(
            validate_command_argv(&argv(&["echo", arg])).is_ok(),
            "{arg} must reach the gates"
        );
    }
    assert!(validate_command_argv(&argv(&["echo", "$RUNNER_TEMP/x"])).is_ok());
    assert!(validate_command_argv(&argv(&["echo", "${x:-y}"])).is_ok());
}

#[test]
fn step_names_reject_github_expressions() {
    assert!(
        shell_step(
            "Custom task ${{secrets.x}}",
            argv(&["true"]),
            BTreeMap::new(),
        )
        .is_err(),
        "expression in step name must fail closed"
    );
    assert!(
        shell_step("Custom task lint", argv(&["true"]), BTreeMap::new()).is_ok(),
        "plain step names stay constructible"
    );
}

#[test]
fn argv_rejects_background_shell_but_keeps_chains_and_urls() {
    for script in ["a & b", "sleep 1&", "& echo hi", "run & sleep"] {
        assert!(
            validate_command_argv(&argv(&["sh", "-c", script])).is_err(),
            "background accepted: {script}"
        );
    }
    for script in [
        "a && b",
        "cmd 2>&1",
        "curl 'https://example.invalid/x?a=1&b=2'",
        "echo done",
    ] {
        assert!(
            validate_command_argv(&argv(&["sh", "-c", script])).is_ok(),
            "legal rejected: {script}"
        );
    }
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
    env.insert(RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40));
    let wired = argv(&[
        "sh",
        "-c",
        &format!(
            "curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256\" | sha256sum -c -"
        ),
    ]);
    let good = acquire_velnor_step(wired.clone(), &env);
    assert!(good.is_ok());
    assert_eq!(
        good.ok().map(|step| step.name),
        Some("Acquire Velnor".to_owned())
    );
    let mut bad_sha = env.clone();
    bad_sha.insert(ASSET_SHA_ENV.to_owned(), "zzz".to_owned());
    assert!(acquire_velnor_step(wired.clone(), &bad_sha).is_err());
    let mut bad_url = env.clone();
    bad_url.insert(
        ASSET_URL_ENV.to_owned(),
        "http://example.invalid/bin".to_owned(),
    );
    assert!(acquire_velnor_step(wired.clone(), &bad_url).is_err());
    let mut no_commit = env.clone();
    no_commit.remove(RELEASE_COMMIT_ENV);
    assert!(acquire_velnor_step(wired.clone(), &no_commit).is_err());
    assert!(acquire_velnor_step(argv(&["fetch", "/tmp/bin"]), &env).is_err());
    assert!(acquire_velnor_step(argv(&["fetch", &staged]), &env).is_err());
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

/// Env validation pins: `A-Z0-9_` keys and single-line clean values
/// pass; every other shape fails with its stable token.
#[test]
fn env_validation_pins_key_and_value_gates_with_tokens() {
    let env = BTreeMap::from([
        ("HELLO".to_owned(), "world".to_owned()),
        ("A1_B2".to_owned(), "x".to_owned()),
    ]);
    assert!(validate_env(&env).is_ok(), "clean env passes");
    assert!(validate_env(&BTreeMap::new()).is_ok(), "empty env passes");
    for (key, token) in [
        ("hello", "bad_env_key:hello"),
        ("", "bad_env_key:"),
        ("HAS-DASH", "bad_env_key:HAS-DASH"),
        ("HAS SPACE", "bad_env_key:HAS SPACE"),
    ] {
        let env = BTreeMap::from([(key.to_owned(), "x".to_owned())]);
        let err = validate_env(&env).expect_err("bad key fails");
        assert!(err.to_string().contains(token), "{key:?}: {err}");
    }
    for value in ["a\nb", "a\rb", "a\0b"] {
        let env = BTreeMap::from([("KEY".to_owned(), value.to_owned())]);
        let err = validate_env(&env).expect_err("bad value fails");
        assert!(
            err.to_string().contains("bad_env_value:KEY"),
            "{value:?}: {err}"
        );
    }
}

/// Argv rejections carry stable tokens, so a refactor dropping one
/// check fails here instead of staying green.
#[test]
fn argv_rejections_carry_stable_tokens() {
    for (items, token) in [
        (vec![], "empty_argv"),
        (argv(&["ok", ""]), "empty_arg"),
        (argv(&["bad\nline"]), "control_char"),
        (argv(&["bad\rline"]), "control_char"),
        (argv(&["echo", "$(evil)"]), "command_substitution"),
        (argv(&["echo", "`evil`"]), "command_substitution"),
        (argv(&["cargo", "install", "x"]), "cargo_install"),
        (argv(&["/usr/bin/cargo", "test"]), "absolute_cargo_path"),
        (argv(&["/cargo", "test"]), "absolute_cargo_path"),
    ] {
        let err = validate_command_argv(&items).expect_err("must reject");
        assert!(err.to_string().contains(token), "{items:?}: {err}");
    }
}
