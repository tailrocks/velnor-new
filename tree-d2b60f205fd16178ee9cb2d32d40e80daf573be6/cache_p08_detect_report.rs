//! Exact recognition for renderer-generated task report wrappers.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use super::{lex, mise_subcommand};

const TASK_ID_ENV: &str = "VELNOR_TASK_ID";
const TASK_DIGEST_ENV: &str = "VELNOR_TASK_DIGEST";
const MATRIX_ID_ENV: &str = "VELNOR_MATRIX_ID";
const MATRIX_KEY_ENV: &str = "VELNOR_MATRIX_KEY";
const START_MS_ENV: &str = "VELNOR_START_MS";
const EXIT_CODE_ENV: &str = "VELNOR_EXIT_CODE";
const REPORT_OP: &str = "write-task-report-v1";
const NEXTEST_TOOL_SPEC_PREFIX: &str = "aqua:nextest-rs/nextest/cargo-nextest@";
const MISE_DATA_DIR_EXPR: &str = "${{ github.workspace }}/.velnor/cache/mise";
const RUSTUP_HOME_EXPR: &str = "${{ github.workspace }}/.velnor/cache/rustup";
const CARGO_HOME_EXPR: &str = "${{ github.workspace }}/.velnor/cache/cargo";

/// Return tools from the exact report wrapper emitted for one typed task.
///
/// The wrapper itself contains generated `if`/`then` control flow. Only its
/// anchored, renderer-compatible form can bypass general shell classification.
/// The inner command must be one direct, literal Mise exec. Any mismatch falls
/// back to the normal fail-closed scanner.
pub(crate) fn report_wrapper_tool_candidates(step: &Step) -> Option<Vec<String>> {
    let StepKind::Shell { run, env } = &step.kind else {
        return None;
    };
    let [shell, flag, script] = run.as_slice() else {
        return None;
    };
    if shell != "sh" || flag != "-c" {
        return None;
    }
    let (stack, kind) = report_identity_matches(step, env)?;
    let matrix_key = env.get(MATRIX_KEY_ENV)?;
    let prelude = format!(
        "{} date +%s%3N > \"$RUNNER_TEMP/velnor/start-{matrix_key}\"; ",
        crate::toolchain_env::credential_unset_prelude()
    );
    let script = script.strip_prefix(&prelude)?;
    let suffix = format!(
        "; code=$?; read -r start_ms rest < \"$RUNNER_TEMP/velnor/start-{matrix_key}\"; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$start_ms\" VELNOR_INTERNAL_OP={REPORT_OP} \"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\"",
        env!("CARGO_PKG_VERSION")
    );
    let inner = script.strip_suffix(&suffix)?;
    let tools = direct_task_tools(inner, stack, kind)?;
    task_tool_homes_match(env, &tools, stack).then_some(tools)
}

/// Bind the wrapper's name, report identity, matrix gate, and credential scrub.
fn report_identity_matches<'a>(
    step: &Step,
    env: &'a BTreeMap<String, String>,
) -> Option<(&'a str, &'a str)> {
    let task_id = env.get(TASK_ID_ENV)?;
    velnor_actions_contract::validate_task_id(task_id).ok()?;
    velnor_actions_contract::validate_digest(env.get(TASK_DIGEST_ENV)?).ok()?;
    let parts: Vec<&str> = task_id.split('/').collect();
    if parts.len() < 5 || parts.first() != Some(&"stack") {
        return None;
    }
    let stack = *parts.get(1)?;
    let base_id = velnor_actions_contract::split_shard_suffix(task_id)
        .map_or(task_id.as_str(), |(base, _, _)| base);
    let base_parts: Vec<&str> = base_id.split('/').collect();
    let kind = *base_parts.get(base_parts.len().checked_sub(2)?)?;
    let base_name = task_step_name(stack, kind)?;
    let expected_name = velnor_actions_contract::split_shard_suffix(task_id).map_or_else(
        || base_name.to_owned(),
        |(_, index, count)| format!("{base_name} (shard {index} of {count})"),
    );
    let expected_condition = format!(
        "!contains(needs.plan.outputs.{}, ',{task_id},')",
        crate::render::COVERED_TASKS_OUTPUT
    );
    if step.name != expected_name || step.condition.as_deref() != Some(expected_condition.as_str())
    {
        return None;
    }
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group(stack, task_id).ok()?;
    if env.get(MATRIX_ID_ENV)? != &matrix_id
        || env.get(MATRIX_KEY_ENV)?
            != &velnor_actions_contract::matrix_key_for_id(&matrix_id).ok()?
    {
        return None;
    }
    for (key, value) in crate::toolchain_env::credential_scrub() {
        if env.get(&key) != Some(&value) {
            return None;
        }
    }
    Some((stack, kind))
}

/// Fixed task display names used by the Rust and OpenTofu adapters.
fn task_step_name(stack: &str, kind: &str) -> Option<&'static str> {
    match (stack, kind) {
        ("rust", "fmt") | ("tofu", "fmt") => Some("Format"),
        ("rust", "clippy") => Some("Clippy"),
        ("rust", "build") => Some("Build test executables"),
        ("rust", "test" | "nextest") => Some("Unit and integration tests"),
        ("rust", "doctest") => Some("Doctests"),
        ("rust", "doc") => Some("Documentation"),
        ("tofu", "init") => Some("Init for validate"),
        ("tofu", "validate") => Some("Validate"),
        _ => None,
    }
}

/// Parse one direct Mise exec and reject dynamic or compound task commands.
fn direct_task_tools(script: &str, stack: &str, kind: &str) -> Option<Vec<String>> {
    if script.is_empty() || script.contains('$') || script.contains('`') {
        return None;
    }
    let detection = lex::detect_script(script, 0);
    if detection.unsupported_mise_syntax || detection.commands.len() != 1 {
        return None;
    }
    let command = detection.commands.first()?;
    if !command.starts_mise() {
        return None;
    }
    let exec_prefix = ["mise", "--no-config", "--no-env", "--no-hooks", "exec"];
    if !command
        .argv
        .iter()
        .take(exec_prefix.len())
        .map(String::as_str)
        .eq(exec_prefix)
    {
        return None;
    }
    let subcommand = mise_subcommand(&command.argv)?;
    if subcommand != exec_prefix.len() - 1 {
        return None;
    }
    let separator = command.argv[subcommand + 1..]
        .iter()
        .position(|word| word == "--")?
        + subcommand
        + 1;
    let tools = command.tool_candidates();
    if tools.is_empty()
        || tools
            .iter()
            .any(|tool| tool.starts_with("mise@unsupported-"))
        || !task_command_matches(stack, kind, &command.argv[separator + 1..], &tools)
    {
        return None;
    }
    Some(tools)
}

/// Bind the child executable and subcommand to the typed task identity.
fn task_command_matches(stack: &str, kind: &str, argv: &[String], tools: &[String]) -> bool {
    if stack == "rust" {
        let uses_mbx = tools.iter().any(|tool| tool.starts_with("mr-boxington@"));
        let expected_program = if uses_mbx { "mbx" } else { "cargo" };
        let Some(program) = argv.first().map(String::as_str) else {
            return false;
        };
        if program != expected_program {
            return false;
        }
        let payload = argv[1..].iter().map(String::as_str).collect::<Vec<_>>();
        return rust_task_command_matches(kind, &payload, tools);
    }
    let prefix = match (stack, kind) {
        ("tofu", "init") => ["tofu", "init"],
        ("tofu", "validate") => ["tofu", "validate"],
        _ => return false,
    };
    argv.iter()
        .take(prefix.len())
        .map(String::as_str)
        .eq(prefix)
}

/// Match one Rust task payload after the Cargo or MBX driver was checked.
fn rust_task_command_matches(kind: &str, argv: &[&str], tools: &[String]) -> bool {
    match kind {
        "fmt" => argv.first() == Some(&"fmt"),
        "clippy" => argv.first() == Some(&"clippy"),
        "build" => test_binary_preparation_matches(argv, tools),
        "test" => argv.first() == Some(&"test"),
        "nextest" => argv.starts_with(&["nextest", "run"]) && has_nextest_tool(tools),
        "doctest" => argv.first() == Some(&"test") && argv.contains(&"--doc"),
        "doc" => argv.first() == Some(&"doc"),
        _ => false,
    }
}

/// Match only the selected test runner's non-executing binary preparation.
fn test_binary_preparation_matches(argv: &[&str], tools: &[String]) -> bool {
    if argv.starts_with(&["nextest", "list"]) {
        return has_nextest_tool(tools)
            && has_pair(argv, "--list-type", "binaries-only")
            && has_profile(argv)
            && has_flags(argv, &["--locked", "--offline"])
            && has_flag(argv, "--manifest-path");
    }
    argv.first() == Some(&"test")
        && has_flags(argv, &["--no-run", "--locked", "--offline"])
        && has_flag(argv, "--manifest-path")
}

fn has_tool_prefix(tools: &[String], prefix: &str) -> bool {
    tools.iter().any(|tool| tool.starts_with(prefix))
}

fn has_nextest_tool(tools: &[String]) -> bool {
    has_tool_prefix(tools, NEXTEST_TOOL_SPEC_PREFIX)
}

fn has_flags(argv: &[&str], flags: &[&str]) -> bool {
    flags.iter().all(|flag| has_flag(argv, flag))
}

fn has_flag(argv: &[&str], flag: &str) -> bool {
    argv.contains(&flag)
}

fn has_pair(argv: &[&str], flag: &str, value: &str) -> bool {
    argv.windows(2).any(|pair| pair == [flag, value])
}

fn has_profile(argv: &[&str]) -> bool {
    argv.windows(2)
        .any(|pair| pair[0] == "--profile" && !pair[1].is_empty())
}

/// Require canonical, isolated Mise and compiler homes for the task route.
fn task_tool_homes_match(env: &BTreeMap<String, String>, tools: &[String], stack: &str) -> bool {
    let policy = [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_DATA_DIR", MISE_DATA_DIR_EXPR),
    ];
    if policy
        .iter()
        .any(|(key, value)| env.get(*key).map(String::as_str) != Some(value))
    {
        return false;
    }
    let rust_versions: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.strip_prefix("rust@"))
        .collect();
    match stack {
        "rust" if rust_versions.len() == 1 => {
            env.get("MISE_RUSTUP_HOME").map(String::as_str) == Some(RUSTUP_HOME_EXPR)
                && env.get("RUSTUP_HOME").map(String::as_str) == Some(RUSTUP_HOME_EXPR)
                && env.get("MISE_CARGO_HOME").map(String::as_str) == Some(CARGO_HOME_EXPR)
                && env.get("CARGO_HOME").map(String::as_str) == Some(CARGO_HOME_EXPR)
                && Some(rust_versions[0]) == env.get("RUSTUP_TOOLCHAIN").map(String::as_str)
        }
        "tofu"
            if rust_versions.is_empty()
                && tools
                    .iter()
                    .filter(|tool| tool.starts_with("opentofu@"))
                    .count()
                    == 1 =>
        {
            [
                "MISE_RUSTUP_HOME",
                "RUSTUP_HOME",
                "MISE_CARGO_HOME",
                "CARGO_HOME",
                "RUSTUP_TOOLCHAIN",
            ]
            .iter()
            .all(|key| !env.contains_key(*key))
        }
        _ => false,
    }
}
