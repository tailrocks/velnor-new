//! Local pin checks against reviewed tool, action, and runner inventories.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::context::{FreshnessContext, py_repr};
use super::inventory::{EXPECTED_ACTIONS, EXPECTED_TOOLS};

const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";
const ACTIONS: &str = "crates/velnor-actions-actionlint/src/actions.rs";
const TOOLS: &str = "crates/velnor-actions-actionlint/src/tools.rs";
const CAPABILITIES: &str = "crates/velnor-actions-actionlint/src/capabilities.rs";
const CONFIG: &str = "crates/velnor-actions-actionlint/src/config.rs";
const RENDERER: &str = "crates/velnor-actions-workflow-renderer/src/render.rs";
pub(super) fn check_local_pins(ctx: &mut FreshnessContext) {
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
                &format!("entry must be an object, got {}", py_repr(Some(&tool))),
            );
            continue;
        }
        let name = tool.get("name").cloned();
        if let Some(name) = name.as_ref().and_then(Value::as_str) {
            seen.insert(name.to_owned());
        }
        let Some((_, constant)) = name.as_ref().and_then(Value::as_str).and_then(|name| {
            EXPECTED_TOOLS
                .iter()
                .find(|(expected, _)| *expected == name)
        }) else {
            ctx.fail_row(
                "local-pin",
                &format!("tool {}", py_repr(name.as_ref())),
                "inventory entry outside the expected tool set",
            );
            continue;
        };
        let actual = ctx.rust_const(CATALOG, constant);
        pin_row(
            ctx,
            &format!("tool {} ({CATALOG}::{constant})", py_repr(name.as_ref())),
            actual.as_deref(),
            tool.get("pinned"),
        );
    }
    for (name, _) in EXPECTED_TOOLS {
        if !seen.contains(name) {
            ctx.fail_row(
                "local-pin",
                &format!("tool {name}"),
                "inventory row missing",
            );
        }
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
                &format!("entry must be an object, got {}", py_repr(Some(&action))),
            );
            continue;
        }
        let key = action.get("key").cloned();
        let key_text = key.as_ref().and_then(Value::as_str).unwrap_or("");
        if !key_text.is_empty() {
            ctx.action_pinned
                .insert(key_text.to_owned(), action.clone());
        }
        let Some((_, prefix)) = EXPECTED_ACTIONS
            .iter()
            .find(|(expected, _)| *expected == key_text)
        else {
            ctx.fail_row(
                "local-pin",
                &format!("action {}", py_repr(key.as_ref())),
                "inventory entry outside the expected action set",
            );
            continue;
        };
        check_action_const(
            ctx,
            key_text,
            prefix,
            "VERSION",
            action.get("pinned_version"),
        );
        check_action_const(ctx, key_text, prefix, "SHA", action.get("pinned_sha"));
    }
    let seen: BTreeSet<String> = ctx.action_pinned.keys().cloned().collect();
    for (key, _) in EXPECTED_ACTIONS {
        if !seen.contains(key) {
            ctx.fail_row(
                "local-pin",
                &format!("action {key}"),
                "inventory row missing",
            );
        }
    }
}

fn check_action_const(
    ctx: &mut FreshnessContext,
    key: &str,
    prefix: &str,
    suffix: &str,
    expected: Option<&Value>,
) {
    let name = format!("{prefix}_{suffix}");
    let actual = ctx.rust_const(ACTIONS, &name);
    pin_row(
        ctx,
        &format!("action {key} {} ({ACTIONS}::{name})", suffix.to_lowercase()),
        actual.as_deref(),
        expected,
    );
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
    let actionlint = ctx.rust_const(CAPABILITIES, "ACTIONLINT_VERSION");
    let actionlint_pin = ctx.tool_pinned.get("actionlint").cloned();
    pin_row(
        ctx,
        "tool actionlint mirror (capabilities.rs)",
        actionlint.as_deref(),
        actionlint_pin.as_ref(),
    );
    let shellcheck = ctx.rust_const(TOOLS, "SHELLCHECK_VERSION");
    let shellcheck_pin = ctx.tool_pinned.get("shellcheck").cloned();
    pin_row(
        ctx,
        "tool shellcheck mirror (tools.rs)",
        shellcheck.as_deref(),
        shellcheck_pin.as_ref(),
    );
    let alint = ctx.rust_const(RENDERER, "ALINT_BINARY_VERSION");
    let alint_pin = ctx
        .action_pinned
        .get("asamarts/alint")
        .and_then(|action| action.get("pinned_version"))
        .cloned();
    pin_row(
        ctx,
        "action asamarts/alint binary mirror (render.rs)",
        alint.as_deref(),
        alint_pin.as_ref(),
    );
}

fn check_runner_pins(ctx: &mut FreshnessContext) {
    ctx.runner = ctx
        .inv
        .get("runner")
        .cloned()
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| json!({}));
    let default = ctx.runner.get("default").cloned();
    let actual = ctx.rust_const(CONFIG, "RUNNER_LABEL_BRIDGE");
    pin_row(
        ctx,
        "runner default (config.rs::RUNNER_LABEL_BRIDGE)",
        actual.as_deref(),
        default.as_ref(),
    );
    ctx.supported = ctx
        .runner
        .get("supported")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if ctx.supported.is_empty() {
        ctx.fail_row(
            "local-pin",
            "runner supported",
            "must be a non-empty label list",
        );
        return;
    }
    for label in ctx.supported.clone() {
        if label.as_str().is_none_or(|label| label.contains("latest")) {
            ctx.fail_row(
                "local-pin",
                "runner supported",
                &format!("unversioned label rejected: {}", py_repr(Some(&label))),
            );
        }
    }
    if ctx
        .supported
        .iter()
        .any(|label| Some(label) == default.as_ref())
    {
        let labels = ctx
            .supported
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",");
        ctx.pass_row("local-pin", "runner supported", &labels);
    } else {
        ctx.fail_row(
            "local-pin",
            "runner supported",
            &format!("default {} not listed", py_repr(default.as_ref())),
        );
    }
}

fn pin_row(
    ctx: &mut FreshnessContext,
    subject: &str,
    actual: Option<&str>,
    expected: Option<&Value>,
) {
    let Some(actual) = actual else { return };
    if expected.and_then(Value::as_str) == Some(actual) {
        ctx.pass_row("local-pin", subject, actual);
    } else {
        ctx.fail_row(
            "local-pin",
            subject,
            &format!(
                "code={} inventory={}",
                py_repr(Some(&json!(actual))),
                py_repr(expected)
            ),
        );
    }
}
