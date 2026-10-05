//! Compare reviewed policy mirrors with the freshness inventory.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::context::{FreshnessContext, parse_iso_date, py_repr};

const EXPECTED_TOOLS: [&str; 13] = [
    "mise",
    "rust",
    "mr-boxington",
    "gh",
    "actionlint",
    "shellcheck",
    "zizmor",
    "nextest",
    "opentofu",
    "release-plz",
    "reuse",
    "python",
    "uv",
];

pub(super) fn check_policy_mirror(ctx: &mut FreshnessContext) {
    let Some(policy) = ctx.policy.clone() else {
        return;
    };
    check_tools_and_runner(ctx, &policy);
    check_actions(ctx, &policy);
}

fn check_tools_and_runner(ctx: &mut FreshnessContext, policy: &Value) {
    let policy_tools = policy.get("tools").unwrap_or(&Value::Null);
    let mut names: BTreeSet<String> = EXPECTED_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    names.extend(object_keys(policy_tools));
    for name in names {
        let wanted = ctx.tool_pinned.get(&name);
        let got = policy_tools.get(&name);
        if !EXPECTED_TOOLS.contains(&name.as_str()) {
            ctx.fail_row(
                "policy-mirror",
                &format!("tool {name}"),
                "policy entry outside the expected tool set",
            );
        } else if got != wanted {
            ctx.fail_row(
                "policy-mirror",
                &format!("tool {name}"),
                &format!("policy={} inventory={}", py_repr(got), py_repr(wanted)),
            );
        } else {
            ctx.pass_row(
                "policy-mirror",
                &format!("tool {name}"),
                &got.map_or_else(|| "None".to_owned(), Value::to_string),
            );
        }
    }
    check_runner_image(ctx, policy);
}

fn check_runner_image(ctx: &mut FreshnessContext, policy: &Value) {
    let images = policy
        .get("github_runner_images")
        .and_then(|value| value.get("linux_x64"))
        .unwrap_or(&Value::Null);
    let default = images.get("default");
    let runner_default = ctx.runner.get("default");
    if default == runner_default {
        ctx.pass_row(
            "policy-mirror",
            "runner default",
            &default.map_or_else(|| "None".to_owned(), Value::to_string),
        );
    } else {
        ctx.fail_row(
            "policy-mirror",
            "runner default",
            &format!(
                "policy={} inventory={}",
                py_repr(default),
                py_repr(runner_default)
            ),
        );
    }
    compare_supported(ctx, images);
}

fn compare_supported(ctx: &mut FreshnessContext, images: &Value) {
    let mut policy_supported = images
        .get("supported")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut inventory_supported = ctx.supported.clone();
    policy_supported.sort_by_key(Value::to_string);
    inventory_supported.sort_by_key(Value::to_string);
    if policy_supported == inventory_supported {
        let joined = images
            .get("supported")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",");
        ctx.pass_row("policy-mirror", "runner supported", &joined);
    } else {
        ctx.fail_row(
            "policy-mirror",
            "runner supported",
            &format!(
                "policy={} inventory={}",
                py_repr(images.get("supported")),
                py_repr(Some(&Value::Array(ctx.supported.clone())))
            ),
        );
    }
}

fn check_actions(ctx: &mut FreshnessContext, policy: &Value) {
    let mut policy_actions: BTreeMap<String, Value> = BTreeMap::new();
    for entry in policy
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !entry.is_object() {
            ctx.fail_row(
                "policy-mirror",
                "actions",
                &format!("entry must be a table, got {}", py_repr(Some(entry))),
            );
            continue;
        }
        check_action_shape(ctx, entry);
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if policy_actions.contains_key(&name) {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {name}"),
                "duplicate entry",
            );
        }
        policy_actions.insert(name, entry.clone());
    }
    compare_actions(ctx, &policy_actions);
}

fn check_action_shape(ctx: &mut FreshnessContext, entry: &Value) {
    for key in object_keys(entry) {
        if !["name", "version", "sha", "reviewed"].contains(&key.as_str()) {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {}", py_repr(entry.get("name"))),
                &format!("unknown key rejected: {key}"),
            );
        }
    }
    let reviewed = entry.get("reviewed").cloned().unwrap_or(Value::Null);
    if parse_iso_date(&reviewed).is_none() {
        ctx.fail_row(
            "policy-mirror",
            &format!("action {}", py_repr(entry.get("name"))),
            &format!(
                "reviewed must be YYYY-MM-DD, got {}",
                py_repr(Some(&reviewed))
            ),
        );
    }
}

fn compare_actions(ctx: &mut FreshnessContext, policy_actions: &BTreeMap<String, Value>) {
    let mut keys: BTreeSet<String> = ctx.action_pinned.keys().cloned().collect();
    keys.extend(policy_actions.keys().cloned());
    for key in keys {
        let Some(wanted) = ctx.action_pinned.get(&key).cloned() else {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {key}"),
                "policy entry without an inventory row",
            );
            continue;
        };
        let Some(got) = policy_actions.get(&key) else {
            ctx.fail_row(
                "policy-mirror",
                &format!("action {key}"),
                "inventory row without a policy entry",
            );
            continue;
        };
        compare_action(ctx, &key, &wanted, got);
    }
}

fn compare_action(ctx: &mut FreshnessContext, key: &str, wanted: &Value, got: &Value) {
    let got_version = got.get("version");
    let got_sha = got.get("sha");
    let pin_version = wanted.get("pinned_version");
    let pin_sha = wanted.get("pinned_sha");
    if got_version != pin_version || got_sha != pin_sha {
        ctx.fail_row(
            "policy-mirror",
            &format!("action {key}"),
            &format!(
                "policy={}@{} inventory={}@{}",
                py_repr(got_version),
                py_repr(got_sha),
                py_repr(pin_version),
                py_repr(pin_sha)
            ),
        );
        return;
    }
    let sha = got_sha.and_then(Value::as_str).unwrap_or("");
    if is_lower_hex_sha(sha) {
        let version = got_version.and_then(Value::as_str).unwrap_or("None");
        ctx.pass_row(
            "policy-mirror",
            &format!("action {key}"),
            &format!("{version}@{sha}"),
        );
    } else {
        ctx.fail_row(
            "policy-mirror",
            &format!("action {key}"),
            &format!("sha must be 40 hex, got {}", py_repr(got_sha)),
        );
    }
}

fn is_lower_hex_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}
