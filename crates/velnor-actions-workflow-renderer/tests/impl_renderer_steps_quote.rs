//! Run-string quoting and join cases (split from step templates).
use std::collections::BTreeMap;
use std::process::Command;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_workflow_renderer::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext, RenderError, STAGED_BINARY_PREFIX,
    checkout_step, join_argv_for_run, plan_step, quote_run_arg, render_workflow_ir, shell_step,
};

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
        velnor_actions_contract::StepKind::Shell { run, .. }
        if run.first().is_some_and(|head| head == "env")
            && run.ends_with(&["cargo".to_owned(), "test".to_owned()])
    ));
    Ok(())
}

#[test]
fn run_quoting_preserves_runner_expansion() -> Result<(), RenderError> {
    assert_eq!(
        quote_run_arg("$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0")?,
        "\"$RUNNER_TEMP\"'/velnor/bin/velnor-actions-0.1.0'"
    );
    assert_eq!(
        quote_run_arg("$RUNNER_TEMP/my dir/tool")?,
        "\"$RUNNER_TEMP\"'/my dir/tool'"
    );
    assert_eq!(
        quote_run_arg("${{ github.run_id }}")?,
        "${{ github.run_id }}"
    );
    assert!(quote_run_arg("${{ github.run_id }").is_err());
    assert_eq!(quote_run_arg("$HOME")?, "\"$HOME\"");
    assert_eq!(quote_run_arg("plain")?, "plain");
    assert_eq!(quote_run_arg("with space")?, "'with space'");
    assert_eq!(quote_run_arg("it's")?, "'it''\\'''s'");
    Ok(())
}

#[test]
fn inline_shell_scripts_quote_whole_for_inner_expansion() -> Result<(), RenderError> {
    let line = join_argv_for_run(&argv(&["sh", "-c", "read sha rest < f && echo \"$sha\""]))?;
    assert_eq!(line, "sh -c 'read sha rest < f && echo \"$sha\"'");
    let quotes = join_argv_for_run(&argv(&["sh", "-c", "cut -d' ' -f1"]))?;
    assert_eq!(quotes, "sh -c 'cut -d'\\'' '\\'' -f1'");
    let direct = join_argv_for_run(&argv(&["mise", "exec", "--", "cargo", "test"]))?;
    assert_eq!(direct, "mise exec -- cargo test");
    Ok(())
}

#[test]
fn unset_wrapped_shell_still_quotes_script_whole() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::toolchain_env::{
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
    assert_eq!(shell_argv(&line, None, "/tmp/Home Space")?, wrapped);
    // An arbitrary `env -u` wrapper still needs to preserve the script
    // argument for the inner shell.
    let foreign = join_argv_for_run(&argv(&["env", "-u", "FOO", "sh", "-c", "echo $x"]));
    let foreign = foreign?;
    assert_eq!(foreign, "env -u FOO sh -c 'echo $x'");
    assert_eq!(
        shell_argv(&foreign, None, "/tmp/Home Space")?,
        argv(&["env", "-u", "FOO", "sh", "-c", "echo $x"])
    );
    Ok(())
}

#[test]
fn typed_argv_quotes_expansions_before_rendering() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        ("$RUNNER_TEMP/my dir/path", "/tmp/Runner Space/my dir/path"),
        (
            r"escaped\ $RUNNER_TEMP/path",
            r"escaped\ /tmp/Runner Space/path",
        ),
        (
            "quoted\"$RUNNER_TEMP/path",
            "quoted\"/tmp/Runner Space/path",
        ),
        (
            r"backslash\\$RUNNER_TEMP/path",
            r"backslash\\/tmp/Runner Space/path",
        ),
    ];
    for (argument, expected) in cases {
        let joined = join_argv_for_run(&argv(&["tool", argument]))?;
        assert_eq!(
            shell_argv(&joined, Some("/tmp/Runner Space"), "/tmp/Home Space")?,
            vec!["tool", expected],
            "argument={argument:?}, joined={joined:?}"
        );
    }
    Ok(())
}

#[test]
fn inline_shell_script_bytes_remain_unchanged_for_the_inner_shell()
-> Result<(), Box<dyn std::error::Error>> {
    let script = format!(
        "mkdir -p \"$RUNNER_TEMP/velnor/bin\" && printf '%s  %s\\n' \"$VELNOR_ASSET_SHA256\" \"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\" | shasum -a 256 -c -",
        env!("CARGO_PKG_VERSION")
    );
    let line = join_argv_for_run(&argv(&["sh", "-c", &script]))?;
    assert_eq!(line, format!("sh -c '{}'", script.replace('\'', "'\\''")));
    assert_eq!(
        shell_argv(&line, None, "/tmp/Home Space")?,
        argv(&["sh", "-c", &script])
    );
    Ok(())
}

pub(crate) fn shell_argv(
    run_line: &str,
    runner_temp: Option<&str>,
    home: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let script = format!("set -- {run_line}; printf '%s\\0' \"$@\"");
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).env_clear().env("HOME", home);
    if let Some(value) = runner_temp {
        command.env("RUNNER_TEMP", value);
    }
    let output = command.output()?;
    assert!(output.status.success(), "shell failed: {run_line:?}");
    split_nul_argv(&output.stdout)
}

fn split_nul_argv(stdout: &[u8]) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    let body = stdout
        .strip_suffix(&[0])
        .ok_or_else(|| std::io::Error::other("missing argv terminator"))?;
    Ok(std::str::from_utf8(body)?
        .split('\0')
        .map(ToOwned::to_owned)
        .collect())
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
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
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
    assert_eq!(
        join_argv_for_run(&argv(&["$RUNNER_TEMP/velnor/bin/x", "--flag"]))?,
        "\"$RUNNER_TEMP\"'/velnor/bin/x' --flag"
    );
    assert!(
        first.contains("\\\"$RUNNER_TEMP\\\"'/velnor/bin/x' --flag"),
        "typed path is not protected in the rendered run value:\n{first}"
    );
    assert!(
        first.contains("\\\"$RUNNER_TEMP\\\"'/velnor/bin/velnor-actions-0.1.0'"),
        "internal path is not protected in the rendered run value:\n{first}"
    );
    Ok(())
}
