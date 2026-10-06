//! Local source pin and inventory mirror checks.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::context::FreshnessContext;

use super::{
    ACTIONS, CAPABILITIES, CATALOG, CONFIG, EXPECTED_ACTIONS, EXPECTED_TOOLS, RENDERER, TOOLS,
};

pub(crate) fn check_local_pins(ctx: &mut FreshnessContext) {
    if reject_duplicate_inventory_rows(ctx) {
        // A malformed inventory must not let duplicate rows overwrite a pin
        // or cause repeated upstream requests later in the run.
        ctx.tools.clear();
        ctx.action_pinned.clear();
        ctx.tool_pinned.clear();
        check_runner_pins(ctx);
        return;
    }
    check_tool_pins(ctx);
    check_action_pins(ctx);
    check_local_mirrors(ctx);
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
        ctx.tools.push(tool.clone());
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

#[cfg(test)]
mod duplicate_tests {
    use serde_json::Value;
    use serde_json::json;

    use super::check_local_pins;
    use crate::context::FreshnessContext;

    #[test]
    fn duplicate_tool_and_action_rows_fail_before_probe_state_is_populated() {
        for inventory in [
            json!({
                "tools": [
                    {"name": "gh", "pinned": "2.102.0"},
                    {"name": "gh", "pinned": "9.9.9"}
                ],
                "actions": [{"key": "actions/checkout", "pinned_version": "v7.0.1"}],
                "runner": {"default": "ubuntu-26.04", "supported": ["ubuntu-26.04"]}
            }),
            json!({
                "tools": [{"name": "gh", "pinned": "2.102.0"}],
                "actions": [
                    {"key": "actions/checkout", "pinned_version": "v7.0.1"},
                    {"key": "actions/checkout", "pinned_version": "v9.9.9"}
                ],
                "runner": {"default": "ubuntu-26.04", "supported": ["ubuntu-26.04"]}
            }),
        ] {
            let mut context = FreshnessContext::new(std::path::PathBuf::new(), true, false);
            context.inv = inventory;

            check_local_pins(&mut context);

            assert!(context.tools.is_empty());
            assert!(context.action_pinned.is_empty());
            assert_eq!(
                context
                    .failures
                    .iter()
                    .filter(|failure| failure.contains("duplicate"))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn unknown_unique_rows_never_enter_upstream_probe_state() {
        let tools = (0..32)
            .map(|index| {
                serde_json::json!({
                    "name": format!("unknown-tool-{index}"),
                    "pinned": "9.9.9",
                    "source": "file:///missing-tool-source"
                })
            })
            .collect::<Vec<_>>();
        let actions = (0..32)
            .map(|index| {
                serde_json::json!({
                    "key": format!("unknown/action-{index}"),
                    "pinned_version": "v9.9.9",
                    "pinned_sha": "0123456789012345678901234567890123456789",
                    "source": "file:///missing-action-source"
                })
            })
            .collect::<Vec<_>>();
        let mut context = FreshnessContext::new(std::path::PathBuf::new(), true, false);
        context.inv = serde_json::json!({
            "tools": tools,
            "actions": actions,
            "runner": {"default": "ubuntu-26.04", "supported": ["ubuntu-26.04"]}
        });

        check_local_pins(&mut context);
        assert!(context.tools.is_empty());
        assert!(context.action_pinned.is_empty());

        crate::probe::check_upstream_probe(&mut context);

        let probe_rows = context
            .output
            .lines()
            .filter_map(|line| line.strip_prefix("row: "))
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|row| row.get("check").and_then(Value::as_str) == Some("upstream-probe"))
            .filter(|row| row.get("subject").and_then(Value::as_str) != Some("runner"))
            .collect::<Vec<_>>();
        assert!(probe_rows.is_empty());
    }
}
