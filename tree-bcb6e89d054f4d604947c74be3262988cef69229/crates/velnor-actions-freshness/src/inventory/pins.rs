//! Local source pin and inventory mirror checks.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::context::FreshnessContext;

use super::{
    ACTIONS, CAPABILITIES, CATALOG, CONFIG, EXPECTED_ACTIONS, EXPECTED_TOOLS, RENDERER, TOOLS,
};

pub(crate) fn check_local_pins(ctx: &mut FreshnessContext) {
    check_tool_pins(ctx);
    check_action_pins(ctx);
    check_local_mirrors(ctx);
    check_runner_pins(ctx);
}

fn check_tool_pins(ctx: &mut FreshnessContext) {
    ctx.tools = ctx
        .inv
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    for tool in ctx.tools.clone() {
        if !tool.is_object() {
            ctx.fail_row(
                "inventory-shape",
                "(inventory tools)",
                &format!("entry must be an object, got {tool}"),
            );
            continue;
        }
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        seen.insert(name.to_owned());
        let Some((_, constant)) = EXPECTED_TOOLS
            .iter()
            .find(|(expected, _)| *expected == name)
        else {
            ctx.fail_row(
                "local-pin",
                &format!("tool {name}"),
                "inventory entry outside the expected tool set",
            );
            continue;
        };
        let actual = ctx.rust_const(CATALOG, constant);
        compare_pin(
            ctx,
            &format!("tool {name} ({CATALOG}::{constant})"),
            actual.as_deref(),
            tool.get("pinned").and_then(Value::as_str),
        );
    }
    for (name, _) in EXPECTED_TOOLS
        .iter()
        .filter(|(name, _)| !seen.contains(*name))
    {
        ctx.fail_row(
            "local-pin",
            &format!("tool {name}"),
            "inventory row missing",
        );
    }
}

fn check_action_pins(ctx: &mut FreshnessContext) {
    let actions = ctx
        .inv
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for action in actions {
        if !action.is_object() {
            ctx.fail_row(
                "inventory-shape",
                "(inventory actions)",
                &format!("entry must be an object, got {action}"),
            );
            continue;
        }
        let key = action
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("<missing>")
            .to_owned();
        ctx.action_pinned.insert(key.clone(), action.clone());
        let Some((_, prefix)) = EXPECTED_ACTIONS
            .iter()
            .find(|(expected, _)| *expected == key)
        else {
            ctx.fail_row(
                "local-pin",
                &format!("action {key}"),
                "inventory entry outside the expected action set",
            );
            continue;
        };
        let version_const = format!("{prefix}_VERSION");
        let sha_const = format!("{prefix}_SHA");
        let version = ctx.rust_const(ACTIONS, &version_const);
        compare_pin(
            ctx,
            &format!("action {key} version ({ACTIONS}::{version_const})"),
            version.as_deref(),
            action.get("pinned_version").and_then(Value::as_str),
        );
        let sha = ctx.rust_const(ACTIONS, &sha_const);
        compare_pin(
            ctx,
            &format!("action {key} sha ({ACTIONS}::{sha_const})"),
            sha.as_deref(),
            action.get("pinned_sha").and_then(Value::as_str),
        );
    }
    let missing = EXPECTED_ACTIONS
        .iter()
        .filter(|(key, _)| !ctx.action_pinned.contains_key(*key))
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    for key in missing {
        ctx.fail_row(
            "local-pin",
            &format!("action {key}"),
            "inventory row missing",
        );
    }
}

fn compare_pin(
    ctx: &mut FreshnessContext,
    subject: &str,
    actual: Option<&str>,
    expected: Option<&str>,
) {
    let Some(actual) = actual else { return };
    if Some(actual) == expected {
        ctx.pass_row("local-pin", subject, actual);
    } else {
        ctx.fail_row(
            "local-pin",
            subject,
            &format!(
                "code='{actual}' inventory='{}'",
                expected.unwrap_or("<missing>")
            ),
        );
    }
}

fn check_local_mirrors(ctx: &mut FreshnessContext) {
    ctx.tool_pinned = ctx
        .tools
        .iter()
        .filter_map(|tool| {
            Some((
                tool.get("name")?.as_str()?.to_owned(),
                tool.get("pinned")?.clone(),
            ))
        })
        .collect();
    let value = ctx.rust_const(CAPABILITIES, "ACTIONLINT_VERSION");
    let expected = ctx
        .tool_pinned
        .get("actionlint")
        .and_then(Value::as_str)
        .map(str::to_owned);
    compare_pin(
        ctx,
        "tool actionlint mirror (capabilities.rs)",
        value.as_deref(),
        expected.as_deref(),
    );
    let value = ctx.rust_const(TOOLS, "SHELLCHECK_VERSION");
    let expected = ctx
        .tool_pinned
        .get("shellcheck")
        .and_then(Value::as_str)
        .map(str::to_owned);
    compare_pin(
        ctx,
        "tool shellcheck mirror (tools.rs)",
        value.as_deref(),
        expected.as_deref(),
    );
    let value = ctx.rust_const(RENDERER, "ALINT_BINARY_VERSION");
    let expected = ctx
        .action_pinned
        .get("asamarts/alint")
        .and_then(|action| action.get("pinned_version"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    compare_pin(
        ctx,
        "action asamarts/alint binary mirror (render.rs)",
        value.as_deref(),
        expected.as_deref(),
    );
}

fn check_runner_pins(ctx: &mut FreshnessContext) {
    ctx.runner = ctx.inv.get("runner").cloned().unwrap_or(Value::Null);
    let default = ctx
        .runner
        .get("default")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let value = ctx.rust_const(CONFIG, "RUNNER_LABEL_BRIDGE");
    compare_pin(
        ctx,
        "runner default (config.rs::RUNNER_LABEL_BRIDGE)",
        value.as_deref(),
        default.as_deref(),
    );
    let supported = ctx
        .runner
        .get("supported")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if supported.is_empty() {
        ctx.fail_row(
            "local-pin",
            "runner supported",
            "must be a non-empty label list",
        );
        return;
    }
    ctx.supported.clear();
    for label in supported {
        match label.as_str() {
            Some(label) if !label.contains("latest") => ctx.supported.push(label.to_owned()),
            _ => ctx.fail_row(
                "local-pin",
                "runner supported",
                &format!("unversioned label rejected: {label}"),
            ),
        }
    }
    if default
        .as_ref()
        .is_some_and(|value| ctx.supported.iter().any(|label| label == value))
    {
        ctx.pass_row("local-pin", "runner supported", &ctx.supported.join(","));
    } else {
        ctx.fail_row(
            "local-pin",
            "runner supported",
            &format!(
                "default {} not listed",
                default.as_deref().unwrap_or("<missing>")
            ),
        );
    }
}
