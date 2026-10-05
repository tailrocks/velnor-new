//! Activated cargo-mutants pin and production-scope checks.

use serde_json::Value;

use super::context::{FreshnessContext, py_repr};
use super::glob::matching_entries;

const MUTANTS: &str = ".cargo/mutants.toml";

pub(super) fn check_validation_tool_pins(ctx: &mut FreshnessContext) {
    let path = ctx.path(MUTANTS);
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row("local-pin", MUTANTS, &format!("unreadable ({error})"));
            return;
        }
    };
    let Some(policy) = ctx.policy.clone() else {
        return;
    };
    check_mutants_pin(ctx, &policy, &text);
    check_scope_matches(ctx, &text);
}

fn check_mutants_pin(ctx: &mut FreshnessContext, policy: &Value, text: &str) {
    let pin = policy
        .get("validation-tools")
        .and_then(|tools| tools.get("cargo-mutants"));
    let Some(pin) = pin else {
        ctx.fail_row(
            "local-pin",
            "validation-tools/cargo-mutants",
            "policy pin missing",
        );
        return;
    };
    let actual = text.lines().find_map(|line| {
        line.strip_prefix("# pinned: cargo-mutants = \"")
            .and_then(|rest| rest.strip_suffix('"'))
    });
    let Some(actual) = actual else {
        ctx.fail_row(
            "local-pin",
            "validation-tools/cargo-mutants",
            &format!("{MUTANTS} lacks a `# pinned: cargo-mutants = \"x\"` line"),
        );
        return;
    };
    if pin.as_str() == Some(actual) {
        ctx.pass_row("local-pin", "validation-tools/cargo-mutants", actual);
    } else {
        ctx.fail_row(
            "local-pin",
            "validation-tools/cargo-mutants",
            &format!(
                "mutants pin={} policy={}",
                py_repr(Some(&Value::String(actual.to_owned()))),
                py_repr(Some(pin))
            ),
        );
    }
}

fn check_scope_matches(ctx: &mut FreshnessContext, text: &str) {
    let patterns = scope_globs(ctx, text);
    for pattern in patterns {
        let hits = match matching_entries(&ctx.root, &pattern) {
            Ok(hits) => hits,
            Err(error) => {
                ctx.fail_row(
                    "local-pin",
                    &format!("{MUTANTS} scope {pattern}"),
                    &format!("glob traversal failed ({error})"),
                );
                continue;
            }
        };
        if hits.iter().any(|entry| entry.is_file) {
            ctx.pass_row(
                "local-pin",
                &format!("{MUTANTS} scope {pattern}"),
                &format!("{} match(es)", hits.len()),
            );
        } else {
            ctx.fail_row(
                "local-pin",
                &format!("{MUTANTS} scope {pattern}"),
                "glob matches no production file",
            );
        }
    }
}

fn scope_globs(ctx: &mut FreshnessContext, text: &str) -> Vec<String> {
    let mut in_scope = false;
    let mut patterns = Vec::new();
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with("examine_globs") {
            in_scope = true;
            continue;
        }
        if in_scope {
            if stripped.starts_with(']') {
                break;
            }
            patterns.extend(quoted_globs(stripped.split('#').next().unwrap_or("")));
        }
    }
    patterns.sort();
    if patterns.is_empty() {
        ctx.fail_row(
            "local-pin",
            &format!("{MUTANTS} examine_globs"),
            "scope is empty",
        );
    }
    patterns
}

fn quoted_globs(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else {
            break;
        };
        values.push(rest[..end].to_owned());
        rest = &rest[end + 1..];
    }
    values
}
