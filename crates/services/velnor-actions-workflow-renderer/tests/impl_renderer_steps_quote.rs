//! Run-string quoting and join cases (split from step templates).
use std::collections::BTreeMap;
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, render_workflow_ir,
};
use velnor_actions_workflow_steps::steps::{
    has_bare_env_expansion, quote_env_path_for_run, quote_run_line_env_paths,
};
use velnor_actions_workflow_steps::{
    RenderError, STAGED_BINARY_PREFIX, checkout_step, join_argv_for_run, plan_step, quote_run_arg,
    shell_step,
};
use velnor_actions_workflow_tree::quote_scalar;

fn pin(name: &str) -> String {
    format!("{name}@{:040x}", 0)
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
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
    // Constructor wraps direct-exec argv in the `env -u` unset prefix;
    // the payload still lands intact behind it.
    assert!(matches!(
        &step.kind,
        velnor_actions_contract_workflow::StepKind::Shell { run, .. }
        if run.first().is_some_and(|head| head == "env")
            && run.ends_with(&["cargo".to_owned(), "test".to_owned()])
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
fn unset_wrapped_shell_still_quotes_script_whole() {
    use velnor_actions_workflow_steps::toolchain_env::{
        CREDENTIAL_UNSET_VARS, with_env_unset_argv,
    };
    let wrapped = with_env_unset_argv(&argv(&["sh", "-c", "read sha rest < f && echo \"$sha\""]));
    let line = join_argv_for_run(&wrapped).expect("wrapped join");
    let mut prefix = vec!["env".to_owned()];
    for var in CREDENTIAL_UNSET_VARS {
        prefix.push("-u".to_owned());
        prefix.push(var.to_owned());
    }
    let want = format!(
        "{} sh -c 'read sha rest < f && echo \"$sha\"'",
        prefix.join(" ")
    );
    assert_eq!(line, want);
    // A foreign `-u` pair stops the prefix: no script position, so
    // the payload quotes as an ordinary arg (`$x` unquoted) instead of
    // a script (`$x` inside single quotes).
    let foreign = join_argv_for_run(&argv(&["env", "-u", "FOO", "sh", "-c", "echo $x"]));
    assert_eq!(foreign.ok().as_deref(), Some("env -u FOO sh -c 'echo '$x"));
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
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: std::collections::BTreeMap::new(),
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
        "plan".to_owned(),
        Job {
            display_name: "Plan".to_owned(),
            runs_on: EMIT_LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::PLAN,
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
    // Payload assertion, not `run:`-anchored: the constructor's
    // `env -u` prefix now heads the run string.
    assert!(
        first.contains("\\\"$RUNNER_TEMP/velnor/bin/x\\\" --flag\""),
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
