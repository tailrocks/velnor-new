//! Activated validation tool pin and mutation-scope checks.

use std::fs;

use toml::Value;

use crate::context::FreshnessContext;
use crate::patterns;

const MUTANTS: &str = ".cargo/mutants.toml";

/// Compare the activated cargo-mutants pin and ensure its source scope exists.
pub(crate) fn check_validation_tool_pins(ctx: &mut FreshnessContext) {
    let text = match fs::read_to_string(ctx.path(MUTANTS)) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row("local-pin", MUTANTS, &format!("unreadable ({error})"));
            return;
        }
    };
    let want = ctx
        .policy
        .as_ref()
        .and_then(|policy| policy.get("validation-tools"))
        .and_then(|tools| tools.get("cargo-mutants"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(want) = want else {
        ctx.fail_row(
            "local-pin",
            "validation-tools/cargo-mutants",
            "policy pin missing",
        );
        return;
    };
    check_mutants_pin(ctx, &text, &want);
    check_scope_matches(ctx, &text);
}

fn check_mutants_pin(ctx: &mut FreshnessContext, text: &str, want: &str) {
    let prefix = "# pinned: cargo-mutants = \"";
    let actual = text.lines().find_map(|line| {
        line.strip_prefix(prefix)
            .and_then(|line| line.strip_suffix('"'))
    });
    match actual {
        None => ctx.fail_row(
            "local-pin",
            "validation-tools/cargo-mutants",
            &format!("{MUTANTS} lacks a `# pinned: cargo-mutants = \"x\"` line"),
        ),
        Some(actual) if actual != want => ctx.fail_row(
            "local-pin",
            "validation-tools/cargo-mutants",
            &format!("mutants pin={actual:?} policy={want:?}"),
        ),
        Some(actual) => ctx.pass_row("local-pin", "validation-tools/cargo-mutants", actual),
    }
}

fn check_scope_matches(ctx: &mut FreshnessContext, text: &str) {
    let document = match toml::from_str::<toml::Table>(text) {
        Ok(document) => document,
        Err(error) => {
            ctx.fail_row("local-pin", MUTANTS, &format!("invalid TOML ({error})"));
            return;
        }
    };
    let Some(globs) = document.get("examine_globs").and_then(Value::as_array) else {
        ctx.fail_row(
            "local-pin",
            &format!("{MUTANTS} examine_globs"),
            "scope is empty",
        );
        return;
    };
    if globs.is_empty() {
        ctx.fail_row(
            "local-pin",
            &format!("{MUTANTS} examine_globs"),
            "scope is empty",
        );
        return;
    }
    for (index, glob) in globs.iter().enumerate() {
        let Some(pattern) = glob.as_str() else {
            ctx.fail_row(
                "local-pin",
                &format!("{MUTANTS} examine_globs"),
                &format!("entry {index} must be a string, got {glob}"),
            );
            continue;
        };
        check_scope_pattern(ctx, pattern);
    }
}

fn check_scope_pattern(ctx: &mut FreshnessContext, pattern: &str) {
    let hits = match patterns::expand(&ctx.root, pattern) {
        Ok(hits) => hits,
        Err(error) => {
            ctx.fail_row("local-pin", &format!("{MUTANTS} scope {pattern}"), &error);
            return;
        }
    };
    let files = hits.iter().filter(|path| path.is_file()).count();
    if files == 0 {
        ctx.fail_row(
            "local-pin",
            &format!("{MUTANTS} scope {pattern}"),
            "glob matches no production file",
        );
    } else {
        ctx.pass_row(
            "local-pin",
            &format!("{MUTANTS} scope {pattern}"),
            &format!("{} match(es)", hits.len()),
        );
    }
}
