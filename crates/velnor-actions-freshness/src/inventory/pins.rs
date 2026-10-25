use std::collections::BTreeSet;

use serde_json::Value;

use crate::context::FreshnessContext;

use super::{ACTIONS, CATALOG, EXPECTED_ACTIONS, EXPECTED_TOOLS, RUNNER_DEFAULT};

#[path = "pins_mirrors.rs"]
mod mirrors;

pub(crate) fn check_local_pins(ctx: &mut FreshnessContext) {
    if reject_duplicate_inventory_rows(ctx) {
        ctx.tools.clear();
        ctx.action_pinned.clear();
        ctx.tool_pinned.clear();
        check_runner_pins(ctx);
        return;
    }
    check_tool_pins(ctx);
    check_action_pins(ctx);
    mirrors::check_local_mirrors(ctx);
    check_runner_pins(ctx);
}

fn reject_duplicate_inventory_rows(ctx: &mut FreshnessContext) -> bool {
    let mut duplicate = false;
    for (section, field) in [("tools", "name"), ("actions", "key")] {
        let entries = ctx
            .inv
            .get(section)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        for entry in entries {
            let Some(key) = entry.get(field).and_then(Value::as_str) else {
                continue;
            };
            if seen.insert(key.to_owned()) {
                continue;
            }
            duplicate = true;
            ctx.fail_row(
                "inventory-shape",
                &format!("(inventory {section})"),
                &format!("duplicate {field} {key:?}"),
            );
        }
    }
    duplicate
}

fn check_tool_pins(ctx: &mut FreshnessContext) {
    let tools = ctx
        .inv
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    ctx.tools.clear();
    let mut seen = BTreeSet::new();
    for tool in tools {
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
        let Some((_, source, constant)) = EXPECTED_TOOLS
            .iter()
            .find(|(expected, _, _)| *expected == name)
        else {
            ctx.fail_row(
                "local-pin",
                &format!("tool {name}"),
                "inventory entry outside the expected tool set",
            );
            continue;
        };
        ctx.tools.push(tool.clone());
        let actual = ctx.rust_const(source, constant);
        compare_pin(
            ctx,
            &format!("tool {name} ({source}::{constant})"),
            actual.as_deref(),
            tool.get("pinned").and_then(Value::as_str),
        );
        if name == "cosign" {
            check_cosign_asset_pins(ctx, &tool);
        }
    }
    for (name, _, _) in EXPECTED_TOOLS
        .iter()
        .filter(|(name, _, _)| !seen.contains(*name))
    {
        ctx.fail_row(
            "local-pin",
            &format!("tool {name}"),
            "inventory row missing",
        );
    }
}

fn check_cosign_asset_pins(ctx: &mut FreshnessContext, tool: &Value) {
    for (platform, bytes_constant, size_field, digest_constant, digest_field) in [
        (
            "Linux x86_64",
            "COSIGN_LINUX_AMD64_BYTES",
            "bytes",
            "COSIGN_LINUX_AMD64_SHA256",
            "sha256",
        ),
        (
            "Linux arm64",
            "COSIGN_LINUX_ARM64_BYTES",
            "arm64_bytes",
            "COSIGN_LINUX_ARM64_SHA256",
            "arm64_sha256",
        ),
    ] {
        let bytes = ctx.rust_usize_const(CATALOG, bytes_constant);
        let inventory_bytes = tool
            .get(size_field)
            .and_then(Value::as_u64)
            .map(|value| value.to_string());
        compare_pin(
            ctx,
            &format!("tool cosign {platform} release size"),
            bytes.as_deref(),
            inventory_bytes.as_deref(),
        );

        let digest = ctx.rust_const(CATALOG, digest_constant);
        compare_pin(
            ctx,
            &format!("tool cosign {platform} release digest"),
            digest.as_deref(),
            tool.get(digest_field).and_then(Value::as_str),
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
        ctx.action_pinned.insert(key.clone(), action.clone());
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

fn check_runner_pins(ctx: &mut FreshnessContext) {
    ctx.runner = ctx.inv.get("runner").cloned().unwrap_or(Value::Null);
    let default = ctx
        .runner
        .get("default")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let value = ctx.rust_const(RUNNER_DEFAULT, "LATEST_RUNNER_LABEL");
    compare_pin(
        ctx,
        "runner default (workflow.rs::LATEST_RUNNER_LABEL)",
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
