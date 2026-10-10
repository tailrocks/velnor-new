use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, toolchain_env};

const REPORT_PREFIX: &str = "; code=$?; VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$s\" VELNOR_INTERNAL_OP=write-task-report-v1 \"";
const REPORT_SUFFIX: &str =
    "\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\"";
const RUNNER_HELPER_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";
const TASK_ID: &str = "VELNOR_TASK_ID";
const TASK_DIGEST: &str = "VELNOR_TASK_DIGEST";
const MATRIX_ID: &str = "VELNOR_MATRIX_ID";
const MATRIX_KEY: &str = "VELNOR_MATRIX_KEY";

pub(super) struct ParsedTask {
    pub(super) argv: Vec<String>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) helper: String,
}

pub(super) fn parse_step(step: &Step) -> Result<Option<ParsedTask>, RenderError> {
    if step.id.is_some() || step.role.is_some() || step.condition.is_none() {
        return Ok(None);
    }
    let StepKind::Shell { run, env } = &step.kind else {
        return Ok(None);
    };
    if !has_task_identity(env) || !has_exact_scrub(env) {
        return Ok(None);
    }
    let Some(script) = unwrap_inline_shell(run) else {
        return Ok(None);
    };
    let prefix = format!(
        "{}s=$(date +%s%3N); ",
        toolchain_env::credential_unset_prelude()
    );
    let Some(body) = script.strip_prefix(&prefix) else {
        return Ok(None);
    };
    let Some(report_at) = body.find(REPORT_PREFIX) else {
        return Ok(None);
    };
    let command = &body[..report_at];
    let report = &body[report_at + REPORT_PREFIX.len()..];
    let Some((helper, suffix)) = report.split_once(REPORT_SUFFIX) else {
        return Ok(None);
    };
    if !suffix.is_empty() || !safe_helper_path(helper) {
        return Ok(None);
    }
    let Some(argv) = parse_static_shell_words(command) else {
        return Ok(None);
    };
    if crate::commands::join_argv_for_run(&argv)? != command {
        return Ok(None);
    }
    if argv.len() < 3 || argv[0] != "mise" || argv[1] != "exec" {
        return Ok(None);
    }
    crate::commands::validate_command_argv(&argv)?;
    let scrub = toolchain_env::credential_scrub();
    let mut task_env = BTreeMap::new();
    for (key, value) in env {
        if scrub.contains_key(key) {
            continue;
        }
        if toolchain_env::is_denied_credential_key(key)
            || toolchain_env::is_denied_endpoint_key(key)
            || key.starts_with("VELNOR_WRAPPER_ARGV_")
        {
            return Ok(None);
        }
        task_env.insert(key.clone(), value.clone());
    }
    if argv
        .iter()
        .enumerate()
        .any(|(index, value)| super::validate_input(&format!("argv_{index}"), value).is_err())
        || task_env
            .iter()
            .any(|(key, value)| super::validate_input(&format!("env_{key}"), value).is_err())
    {
        return Ok(None);
    }
    Ok(Some(ParsedTask {
        argv,
        env: task_env,
        helper: helper.to_owned(),
    }))
}

fn has_task_identity(env: &BTreeMap<String, String>) -> bool {
    [TASK_ID, TASK_DIGEST, MATRIX_ID, MATRIX_KEY]
        .iter()
        .all(|key| env.get(*key).is_some_and(|value| !value.is_empty()))
}

fn has_exact_scrub(env: &BTreeMap<String, String>) -> bool {
    toolchain_env::credential_scrub()
        .iter()
        .all(|(key, value)| env.get(key) == Some(value))
}

fn unwrap_inline_shell(run: &[String]) -> Option<&str> {
    if run.len() != 3 || run[0] != "sh" || run[1] != "-c" {
        return None;
    }
    run.get(2).map(String::as_str)
}

fn safe_helper_path(helper: &str) -> bool {
    helper
        .strip_prefix(RUNNER_HELPER_PREFIX)
        .is_some_and(|version| {
            version.contains('.')
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

/// Decode only `join_argv_for_run`'s static shell-word subset.
///
/// Quoted static data, including the encoder's `\\'` apostrophe escape,
/// round-trips. Expansions, operators, and unsupported shell syntax fail
/// closed and leave the original typed step untouched.
fn parse_static_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = command.bytes();
    while let Some(byte) = chars.next() {
        match byte {
            b'\'' if quoted => quoted = false,
            b'\'' => {
                quoted = true;
                started = true;
            }
            b' ' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            b'\\' if !quoted => {
                if chars.next()? != b'\'' {
                    return None;
                }
                word.push('\'');
                started = true;
            }
            byte if quoted => {
                if !byte.is_ascii() {
                    return None;
                }
                word.push(char::from(byte));
                started = true;
            }
            byte if plain_shell_byte(byte) => {
                word.push(char::from(byte));
                started = true;
            }
            _ => return None,
        }
    }
    if quoted {
        return None;
    }
    if started {
        words.push(word);
    }
    (!words.is_empty() && words.iter().all(|word| !word.is_empty())).then_some(words)
}

fn plain_shell_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte)
}
