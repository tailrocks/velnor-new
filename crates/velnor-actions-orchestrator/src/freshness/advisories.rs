//! Local advisory deny policy and optional bounded Cargo Deny scan.

use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use super::context::{FreshnessContext, py_repr};
#[path = "advisories_process.rs"]
mod process;
#[path = "advisories_readers.rs"]
mod readers;

const LIVE_SCAN_LIMIT: Duration = Duration::from_secs(180);

pub(super) fn check_advisories(ctx: &mut FreshnessContext) {
    check_deny_policy(ctx);
    if ctx.with_advisories {
        check_live_advisories(ctx);
    } else {
        ctx.info_row(
            "advisories",
            "live scan",
            "runs as the CI Cargo Deny job; --with-advisories runs it here",
        );
    }
}

fn check_deny_policy(ctx: &mut FreshnessContext) {
    let loaded = std::fs::read_to_string(ctx.path("deny.toml"))
        .map_err(|error| error.to_string())
        .and_then(|text| toml::from_str::<toml::Value>(&text).map_err(|error| error.to_string()));
    let deny = match loaded {
        Ok(deny) => deny,
        Err(error) => {
            ctx.fail_row("advisories", "deny.toml", &format!("unreadable ({error})"));
            return;
        }
    };
    let Some(advisories) = deny.get("advisories").and_then(toml::Value::as_table) else {
        ctx.fail_row("advisories", "deny.toml", "[advisories] table missing");
        return;
    };
    let ignored = advisories.get("ignore");
    if is_truthy(ignored) {
        match ignored {
            Some(toml::Value::Array(entries)) => {
                for entry in entries {
                    ctx.fail_row(
                        "advisories",
                        &toml_string(entry),
                        "ignored advisory must be a policy exception instead",
                    );
                }
            }
            Some(toml::Value::String(entries)) => {
                for entry in entries.chars() {
                    ctx.fail_row(
                        "advisories",
                        &entry.to_string(),
                        "ignored advisory must be a policy exception instead",
                    );
                }
            }
            Some(value) => ctx.fail_row(
                "advisories",
                "deny.toml",
                &format!(
                    "advisories.ignore must be iterable, got {}",
                    py_repr(Some(&toml_json(value)))
                ),
            ),
            None => ctx.pass_row("advisories", "deny ignore list", "empty"),
        }
    } else {
        ctx.pass_row("advisories", "deny ignore list", "empty");
    }
}

fn check_live_advisories(ctx: &mut FreshnessContext) {
    let mut command = Command::new("cargo-deny");
    command
        .args(["check", "advisories"])
        .current_dir(&ctx.root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::isolate_process_tree(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ctx.fail_row(
                "advisories",
                "live scan",
                "--with-advisories requested but cargo-deny is not on PATH",
            );
            return;
        }
        Err(error) => {
            ctx.fail_row("advisories", "live scan", &format!("tool failed ({error})"));
            return;
        }
    };
    let output = match run_live_child(
        &mut child,
        process::RunLimits {
            runtime: LIVE_SCAN_LIMIT,
            output_drain: process::OUTPUT_DRAIN_LIMIT,
            cleanup: process::CLEANUP_LIMIT,
        },
    ) {
        Ok(output) => output,
        Err(error) => {
            ctx.fail_row("advisories", "live scan", &format!("tool failed ({error})"));
            return;
        }
    };
    report_live_result(ctx, output.status, &output.streams);
}

fn run_live_child(
    child: &mut Child,
    limits: process::RunLimits,
) -> io::Result<process::LiveOutput> {
    #[cfg(unix)]
    {
        process::run_bounded(child, limits)
    }
    #[cfg(not(unix))]
    {
        readers::run_threaded_bounded(child, limits)
    }
}

fn report_live_result(
    ctx: &mut FreshnessContext,
    status: Option<ExitStatus>,
    output: &(Vec<u8>, Vec<u8>),
) {
    let Some(status) = status else {
        ctx.fail_row(
            "advisories",
            "live scan",
            "tool failed (timed out after 180 seconds)",
        );
        return;
    };
    if status.success() {
        ctx.pass_row(
            "advisories",
            "live scan",
            "cargo deny check advisories: no findings",
        );
    } else {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.0),
            String::from_utf8_lossy(&output.1)
        );
        let tail = last_chars(&combined, 500);
        ctx.fail_row(
            "advisories",
            "live scan",
            &format!(
                "cargo deny reported findings; run `cargo deny check advisories` for evidence: {}",
                py_repr(Some(&json!(tail)))
            ),
        );
    }
}

fn last_chars(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(limit)).collect()
}

fn is_truthy(value: Option<&toml::Value>) -> bool {
    match value {
        None | Some(toml::Value::Boolean(false)) => false,
        Some(toml::Value::Integer(number)) => *number != 0,
        Some(toml::Value::Float(number)) => *number != 0.0,
        Some(toml::Value::String(value)) => !value.is_empty(),
        Some(toml::Value::Array(values)) => !values.is_empty(),
        Some(toml::Value::Table(values)) => !values.is_empty(),
        Some(toml::Value::Datetime(_) | toml::Value::Boolean(true)) => true,
    }
}

fn toml_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn toml_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

#[cfg(all(test, unix))]
#[path = "advisories_bounded_tests.rs"]
mod advisories_bounded_tests;
