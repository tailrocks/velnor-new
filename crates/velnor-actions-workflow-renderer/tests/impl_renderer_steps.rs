//! Fixed step-template and command-validation cases.
use std::collections::BTreeMap;
use velnor_actions_contract::{Concurrency, Job, Permissions, Trigger, WorkflowIr, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    has_bare_env_expansion, quote_env_path_for_run, quote_run_line_env_paths,
};
use velnor_actions_workflow_renderer::{
    ASSET_SHA_ENV, ASSET_URL_ENV, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext,
    RenderError, STAGED_BINARY_PREFIX, acquire_velnor_step, action_step, checkout_step,
    internal_step, join_argv_for_run, merge_step, plan_step, quote_run_arg, quote_scalar,
    render_workflow_ir, scan_for_private_subcommands, shell_step, validate_command_argv,
    validate_uses,
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
fn inline_shell_scripts_quote_whole_for_inner_expansion() {
    let line = join_argv_for_run(&argv(&["sh", "-c", "read sha rest < f && echo \"$sha\""]));
    assert_eq!(
        line.ok().as_deref(),
        Some("sh -c 'read sha rest < f && echo \"$sha\"'")
    );
    let quotes = join_argv_for_run(&argv(&["sh", "-c", "cut -d' ' -f1"]));
    assert_eq!(
        quotes.ok().as_deref(),
        Some("sh -c 'cut -d'\\'' '\\'' -f1'")
    );
    let direct = join_argv_for_run(&argv(&["mise", "exec", "--", "cargo", "test"]));
    assert_eq!(direct.ok().as_deref(), Some("mise exec -- cargo test"));
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
    let wired = argv(&[
        "sh",
        "-c",
        &format!(
            "curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256\" | sha256sum -c -"
        ),
    ]);
    let good = acquire_velnor_step(wired.clone(), env.clone());
    assert!(good.is_ok());
    assert_eq!(
        good.ok().map(|step| step.name),
        Some("Acquire Velnor".to_owned())
    );
    let mut bad_sha = env.clone();
    bad_sha.insert(ASSET_SHA_ENV.to_owned(), "zzz".to_owned());
    assert!(acquire_velnor_step(wired.clone(), bad_sha).is_err());
    let mut bad_url = env.clone();
    bad_url.insert(
        ASSET_URL_ENV.to_owned(),
        "http://example.invalid/bin".to_owned(),
    );
    assert!(acquire_velnor_step(wired.clone(), bad_url).is_err());
    assert!(acquire_velnor_step(argv(&["fetch", "/tmp/bin"]), env.clone()).is_err());
    assert!(acquire_velnor_step(argv(&["fetch", &staged]), env).is_err());
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
fn env_paths_quote_for_run_without_word_splitting() {
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let quoted = quote_env_path_for_run(&staged);
    assert_eq!(quoted, "\"$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0\"");
    assert_eq!(quote_env_path_for_run(&quoted), quoted);
    assert_eq!(quote_env_path_for_run(&staged), quoted);
    assert_eq!(quote_env_path_for_run("cargo"), "cargo");
    assert_eq!(
        quote_env_path_for_run("${{ github.run_id }}"),
        "${{ github.run_id }}"
    );
    assert_eq!(
        quote_env_path_for_run("${RUNNER_TEMP}/tool"),
        "\"${RUNNER_TEMP}/tool\""
    );
}

#[test]
fn requote_leaves_quoted_words_untouched() {
    let bare = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0";
    assert_eq!(
        quote_run_line_env_paths(bare),
        "\"$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0\""
    );
    let mixed = "sh -c 'read sha rest < f && printf \"%s\" \"$sha\"' $RUNNER_TEMP/x";
    assert_eq!(
        quote_run_line_env_paths(mixed),
        "sh -c 'read sha rest < f && printf \"%s\" \"$sha\"' \"$RUNNER_TEMP/x\""
    );
    let script = "sh -c 'mkdir '$RUNNER_TEMP'/x && read sha rest'";
    assert_eq!(quote_run_line_env_paths(script), script);
}

#[test]
fn emitted_run_lines_carry_no_bare_env_expansion() {
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let run_value = quote_env_path_for_run(&staged);
    assert!(!has_bare_env_expansion(&run_value));
    assert!(has_bare_env_expansion(&staged));
    assert!(has_bare_env_expansion("run: $RUNNER_TEMP/x"));
    assert!(has_bare_env_expansion("run: ${RUNNER_TEMP}/x"));
    assert!(!has_bare_env_expansion("run: \"$RUNNER_TEMP/x\""));
    assert!(!has_bare_env_expansion("run: ${{ github.run_id }}"));
    assert!(!has_bare_env_expansion("run: 'literal $HOME stays put'"));
    let yaml_scalar = quote_scalar(&run_value);
    assert_eq!(
        yaml_scalar,
        "\"\\\"$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0\\\"\""
    );
    assert_eq!(unescape_yaml_double(&yaml_scalar), Some(run_value));
}

/// Minimal `"`-scalar decoder proving the YAML round-trip.
fn unescape_yaml_double(scalar: &str) -> Option<String> {
    let inner = scalar.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                _ => return None,
            }
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

const EMIT_VERSION: &str = "0.1.0";
const EMIT_LABEL: &str = "ubuntu-26.04";

fn emit_ctx() -> RenderContext {
    RenderContext {
        generator_version: EMIT_VERSION.to_owned(),
        runs_on: EMIT_LABEL.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{EMIT_VERSION}"),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: pin("actions/checkout"),
        policy_commands: Vec::new(),
        candidate: None,
        preseed: false,
    }
}

fn emit_triggers() -> Trigger {
    Trigger {
        pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
            .iter()
            .map(ToString::to_string)
            .collect(),
        push_branches: vec!["main".to_owned()],
        merge_group: true,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn emit_concurrency() -> Concurrency {
    Concurrency {
        group: CONCURRENCY_GROUP.to_owned(),
        cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
    }
}

#[test]
fn rendered_run_steps_quote_runner_temp_paths() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "velnor-plan".to_owned(),
        Job {
            display_name: "Velnor Plan".to_owned(),
            runs_on: EMIT_LABEL.to_owned(),
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![
                checkout_step(&pin("actions/checkout"))?,
                shell_step(
                    "Run tool",
                    argv(&["$RUNNER_TEMP/velnor/bin/x", "--flag"]),
                    BTreeMap::new(),
                )?,
                plan_step(),
            ],
        },
    );
    let ir = WorkflowIr {
        name: "CI".to_owned(),
        triggers: emit_triggers(),
        permissions: Permissions::default(),
        concurrency: emit_concurrency(),
        jobs,
    };
    let ctx = emit_ctx();
    let first = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    let second = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert_eq!(first, second);
    assert!(
        first.contains("run: \"\\\"$RUNNER_TEMP/velnor/bin/x\\\" --flag\""),
        "quoted shell run missing:\n{first}"
    );
    assert!(
        first.contains("run: \"\\\"$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0\\\"\""),
        "quoted internal run missing:\n{first}"
    );
    for line in first.lines() {
        if line.trim_start().starts_with("run:") {
            assert!(!has_bare_env_expansion(line), "bare expansion in {line:?}");
        }
    }
    let quoted = "\"$RUNNER_TEMP/velnor/bin/x\" --flag";
    assert_eq!(
        quote_run_line_env_paths("$RUNNER_TEMP/velnor/bin/x --flag"),
        quoted
    );
    assert_eq!(quote_run_line_env_paths(quoted), quoted);
    Ok(())
}

#[test]
fn private_subcommand_scan_finds_hidden_tokens() {
    assert!(scan_for_private_subcommands("run: velnor-actions plan").is_ok());
    assert!(scan_for_private_subcommands("velnor-actions __internal").is_err());
    assert!(scan_for_private_subcommands("velnor-actions run x").is_err());
    assert!(scan_for_private_subcommands("prefix __internal suffix").is_err());
    assert!(action_step("x", "velnor-actions __y", BTreeMap::new()).is_err());
}
