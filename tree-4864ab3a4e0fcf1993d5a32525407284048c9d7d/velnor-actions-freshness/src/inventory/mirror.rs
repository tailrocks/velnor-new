//! Reviewed version-policy mirror checks.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use toml::Value as TomlValue;

use crate::context::{FreshnessContext, parse_iso_date};

use super::EXPECTED_TOOLS;

pub(crate) fn check_policy_mirror(ctx: &mut FreshnessContext) {
    let Some(policy) = ctx.policy.clone() else {
        return;
    };
    let policy_tools = policy.get("tools").and_then(TomlValue::as_table);
    let names = EXPECTED_TOOLS
        .iter()
        .map(|(name, _)| *name)
        .chain(
            policy_tools
                .into_iter()
                .flat_map(|table| table.keys().map(String::as_str)),
        )
        .collect::<BTreeSet<&str>>();
    for name in names {
        let want = ctx.tool_pinned.get(name).and_then(Value::as_str);
        let got = policy_tools
            .and_then(|table| table.get(name))
            .and_then(TomlValue::as_str);
        if !EXPECTED_TOOLS.iter().any(|(expected, _)| *expected == name) {
            ctx.fail_row(
                "policy-mirror",
                &format!("tool {name}"),
                "policy entry outside the expected tool set",
            );
        } else if got == want {
            ctx.pass_row(
                "policy-mirror",
                &format!("tool {name}"),
                got.unwrap_or("<missing>"),
            );
        } else {
            ctx.fail_row(
                "policy-mirror",
                &format!("tool {name}"),
                &format!("policy={got:?} inventory={want:?}"),
            );
        }
    }
    check_policy_runner(ctx, &policy);
    check_policy_actions(ctx, &policy);
}

fn check_policy_runner(ctx: &mut FreshnessContext, policy: &TomlValue) {
    let images = policy
        .get("github_runner_images")
        .and_then(|value| value.get("linux_x64"));
    let default = images
        .and_then(|value| value.get("default"))
        .and_then(TomlValue::as_str)
        .map(str::to_owned);
    let inventory_default = ctx
        .runner
        .get("default")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if default == inventory_default {
        ctx.pass_row(
            "policy-mirror",
            "runner default",
            default.as_deref().unwrap_or("<missing>"),
        );
    } else {
        ctx.fail_row(
            "policy-mirror",
            "runner default",
            &format!("policy={default:?} inventory={inventory_default:?}"),
        );
    }
    let Some(supported) = images
        .and_then(|value| value.get("supported"))
        .and_then(TomlValue::as_array)
    else {
        ctx.fail_row(
            "policy-mirror",
            "runner supported",
            "must be an array of string labels",
        );
        return;
    };
    let mut policy_supported = BTreeSet::new();
    let mut valid = true;
    for (index, label) in supported.iter().enumerate() {
        let Some(label) = label.as_str() else {
            ctx.fail_row(
                "policy-mirror",
                "runner supported",
                &format!("entry {index} must be a string, got {label}"),
            );
            valid = false;
            continue;
        };
        if !policy_supported.insert(label.to_owned()) {
            ctx.fail_row(
                "policy-mirror",
                "runner supported",
                &format!("duplicate label: {label}"),
            );
            valid = false;
        }
    }
    let inventory_supported = ctx.supported.iter().cloned().collect::<BTreeSet<_>>();
    if valid && policy_supported == inventory_supported {
        ctx.pass_row(
            "policy-mirror",
            "runner supported",
            &ctx.supported.join(","),
        );
    } else if valid {
        ctx.fail_row(
            "policy-mirror",
            "runner supported",
            "policy supported labels differ from inventory",
        );
    }
}

fn check_policy_actions(ctx: &mut FreshnessContext, policy: &TomlValue) {
    let entries = policy
        .get("actions")
        .and_then(TomlValue::as_array)
        .cloned()
        .unwrap_or_default();
    let mut by_name = BTreeMap::new();
    for entry in entries {
        let Some(table) = entry.as_table() else {
            ctx.fail_row(
                "policy-mirror",
                "actions",
                &format!("entry must be a table, got {entry}"),
            );
            continue;
        };
        let name = table
            .get("name")
            .and_then(TomlValue::as_str)
            .unwrap_or("<missing>")
            .to_owned();
        for key in table
            .keys()
            .filter(|key| !["name", "version", "sha", "reviewed"].contains(&key.as_str()))
        {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {name}"),
                &format!("unknown key rejected: {key}"),
            );
        }
        let reviewed = table
            .get("reviewed")
            .and_then(TomlValue::as_str)
            .and_then(parse_iso_date);
        if reviewed.is_none() {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {name}"),
                "reviewed must be YYYY-MM-DD",
            );
        }
        if by_name.insert(name.clone(), entry).is_some() {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {name}"),
                "duplicate entry",
            );
        }
    }
    let names = ctx
        .action_pinned
        .keys()
        .cloned()
        .chain(by_name.keys().cloned())
        .collect::<BTreeSet<_>>();
    for name in names {
        let inventory = ctx.action_pinned.get(&name).cloned();
        let policy = by_name.get(&name).cloned();
        match (inventory, policy) {
            (None, Some(_)) => ctx.fail_row(
                "policy-mirror",
                &format!("action {name}"),
                "policy entry without an inventory row",
            ),
            (Some(_), None) => ctx.fail_row(
                "policy-mirror",
                &format!("action {name}"),
                "inventory row without a policy entry",
            ),
            (Some(inventory), Some(policy)) => {
                compare_policy_action(ctx, &name, &inventory, &policy);
            }
            (None, None) => {}
        }
    }
}

fn compare_policy_action(
    ctx: &mut FreshnessContext,
    name: &str,
    inventory: &Value,
    policy: &TomlValue,
) {
    let version = policy.get("version").and_then(TomlValue::as_str);
    let sha = policy.get("sha").and_then(TomlValue::as_str);
    let pinned_version = inventory.get("pinned_version").and_then(Value::as_str);
    let pinned_sha = inventory.get("pinned_sha").and_then(Value::as_str);
    if version != pinned_version || sha != pinned_sha {
        ctx.fail_row(
            "policy-mirror",
            &format!("action {name}"),
            &format!("policy={version:?}@{sha:?} inventory={pinned_version:?}@{pinned_sha:?}"),
        );
    } else if !sha.is_some_and(is_sha) {
        ctx.fail_row(
            "policy-mirror",
            &format!("action {name}"),
            &format!("sha must be 40 hex, got {sha:?}"),
        );
    } else {
        ctx.pass_row(
            "policy-mirror",
            &format!("action {name}"),
            &format!("{}@{}", version.unwrap_or(""), sha.unwrap_or("")),
        );
    }
}

fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
