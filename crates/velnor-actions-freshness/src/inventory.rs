//! Inventory shape, compiled pin, policy header, and mirror checks.

mod mirror;

mod pins;

use std::fs;

use std::path::Path;

use serde_json::Value;

use toml::Value as TomlValue;

use crate::context::FreshnessContext;

const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";
const ACTIONS: &str = "crates/velnor-actions-actionlint/src/actions.rs";
const TOOLS: &str = "crates/velnor-actions-actionlint/src/tools.rs";
const CAPABILITIES: &str = "crates/velnor-actions-actionlint/src/capabilities.rs";
const CONFIG: &str = "crates/velnor-actions-actionlint/src/config.rs";
const RENDERER: &str = "crates/velnor-actions-workflow-renderer/src/render.rs";

pub(crate) const EXPECTED_TOOLS: &[(&str, &str)] = &[
    ("mise", "MISE_VERSION"),
    ("rust", "RUST_VERSION"),
    ("mr-boxington", "MR_BOXINGTON_VERSION"),
    ("gh", "GH_VERSION"),
    ("actionlint", "ACTIONLINT_VERSION"),
    ("shellcheck", "SHELLCHECK_VERSION"),
    ("zizmor", "ZIZMOR_VERSION"),
    ("nextest", "NEXTEST_VERSION"),
    ("opentofu", "OPENTOFU_VERSION"),
    ("release-plz", "RELEASE_PLZ_VERSION"),
];

pub(crate) const EXPECTED_ACTIONS: &[(&str, &str)] = &[
    ("jdx/mise-action", "MISE_ACTION"),
    ("actions/checkout", "CHECKOUT_ACTION"),
    ("actions/download-artifact", "DOWNLOAD_ARTIFACT_ACTION"),
    ("actions/upload-artifact", "UPLOAD_ARTIFACT_ACTION"),
    ("actions/cache/restore", "CACHE_ACTION"),
    ("actions/cache/save", "CACHE_ACTION"),
    ("jdx/mr-boxington-action", "MR_BOXINGTON_ACTION"),
    ("asamarts/alint", "ALINT_ACTION"),
    (
        "aws-actions/configure-aws-credentials",
        "AWS_CREDENTIALS_ACTION",
    ),
];
/// Load the reviewed JSON inventory, recording shape errors as gate rows.
pub(crate) fn load_inventory(ctx: &mut FreshnessContext) -> bool {
    let path = ctx.path(".velnor/freshness-inventory.json");
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(error) => {
            ctx.fail_row(
                "inventory-shape",
                ".velnor/freshness-inventory.json",
                &format!("unreadable ({error})"),
            );
            return false;
        }
    };
    let inventory: Value = match serde_json::from_slice(&data) {
        Ok(inventory) => inventory,
        Err(error) => {
            ctx.fail_row(
                "inventory-shape",
                ".velnor/freshness-inventory.json",
                &format!("unreadable ({error})"),
            );
            return false;
        }
    };
    if !inventory.is_object() {
        ctx.fail_row(
            "inventory-shape",
            "root",
            "inventory root must be an object",
        );
        return false;
    }
    ctx.inv = inventory;
    check_inventory_shape(ctx);
    true
}

fn check_inventory_shape(ctx: &mut FreshnessContext) {
    let known = [
        "schema",
        "check_interval_hours",
        "max_exception_days",
        "checked_at",
        "tools",
        "actions",
        "runner",
        "exceptions",
        "temporary_holds",
    ];
    if let Some(object) = ctx.inv.as_object() {
        let extra = object
            .keys()
            .filter(|key| !known.contains(&key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for key in extra {
            ctx.info_row("inventory-shape", &key, "unrecognized top-level key");
        }
    }
    for key in ["tools", "actions", "exceptions", "temporary_holds"] {
        if let Some(value) = ctx.inv.get(key)
            && !value.is_array()
        {
            ctx.fail_row(
                "inventory-shape",
                key,
                &format!("must be an array, got {value}"),
            );
        }
    }
    let schema = ctx.inv.get("schema").and_then(Value::as_i64);
    if schema != Some(1) {
        let got = display_json(ctx.inv.get("schema"));
        ctx.fail_row(
            "inventory-shape",
            "schema",
            &format!("must be 1, got {got}"),
        );
    }
    ctx.interval = positive_integer(ctx, "check_interval_hours", 24);
    ctx.max_days = positive_integer(ctx, "max_exception_days", 14);
    ctx.top_checked = match ctx.inv.get("checked_at") {
        Some(Value::String(stamp)) => Some(stamp.clone()),
        Some(value) => {
            ctx.fail_row(
                "inventory-shape",
                "checked_at",
                &format!("must be a string when present, got {value}"),
            );
            None
        }
        None => None,
    };
    if let Some(stamp) = &ctx.top_checked
        && crate::context::parse_timestamp(stamp).is_none()
    {
        ctx.fail_row(
            "inventory-shape",
            "checked_at",
            &format!("malformed timestamp {stamp:?}"),
        );
    }
}

fn positive_integer(ctx: &mut FreshnessContext, key: &str, default: i64) -> i64 {
    let value = ctx.inv.get(key).and_then(Value::as_i64);
    match value {
        Some(value) if value > 0 => value,
        _ => {
            let got = display_json(ctx.inv.get(key));
            ctx.fail_row(
                "inventory-shape",
                key,
                &format!("must be a positive int, got {got}"),
            );
            default
        }
    }
}

/// Load the repository's TOML version policy.
pub(crate) fn load_policy(ctx: &mut FreshnessContext) {
    let relative = ".velnor/version-policy.toml";
    match fs::read_to_string(ctx.path(relative))
        .map_err(|error| error.to_string())
        .and_then(|text| toml::from_str::<TomlValue>(&text).map_err(|error| error.to_string()))
    {
        Ok(policy) if policy.is_table() => ctx.policy = Some(policy),
        Ok(_) => ctx.fail_row("policy-header", relative, "root must be a table"),
        Err(error) => ctx.fail_row(
            "policy-header",
            "version-policy.toml",
            &format!("unreadable ({error})"),
        ),
    }
}

/// Validate fixed policy header and prevent weakening the cadence limits.
pub(crate) fn check_policy_header(ctx: &mut FreshnessContext) {
    let Some(policy) = ctx.policy.clone() else {
        return;
    };
    let allowed = [
        "schema",
        "channel",
        "registry",
        "check_interval_hours",
        "max_exception_days",
        "tools",
        "github_runner_images",
        "actions",
        "validation-tools",
    ];
    let extras = policy
        .as_table()
        .into_iter()
        .flat_map(|table| table.keys())
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for key in extras {
        ctx.fail_row("policy-header", &key, "unknown key rejected");
    }
    check_policy_value(ctx, &policy, "schema", &TomlValue::Integer(1));
    check_policy_value(
        ctx,
        &policy,
        "channel",
        &TomlValue::String("stable".to_owned()),
    );
    check_policy_value(
        ctx,
        &policy,
        "check_interval_hours",
        &TomlValue::Integer(ctx.interval),
    );
    check_policy_value(
        ctx,
        &policy,
        "max_exception_days",
        &TomlValue::Integer(ctx.max_days),
    );
    check_registry(ctx, &policy);
    if toml_int(&policy, "check_interval_hours").is_some_and(|value| value > 24) {
        ctx.fail_row(
            "policy-header",
            "check_interval_hours",
            "weakens policy: must be <= 24",
        );
    }
    if toml_int(&policy, "max_exception_days").is_some_and(|value| value > 14) {
        ctx.fail_row(
            "policy-header",
            "max_exception_days",
            "weakens policy: must be <= 14",
        );
    }
}

fn check_policy_value(ctx: &mut FreshnessContext, policy: &TomlValue, key: &str, want: &TomlValue) {
    let got = policy.get(key);
    if got == Some(want) {
        ctx.pass_row("policy-header", key, &format!("{want}"));
    } else {
        ctx.fail_row(
            "policy-header",
            key,
            &format!(
                "got={} want={want}",
                got.map_or_else(|| "<missing>".to_owned(), ToString::to_string)
            ),
        );
    }
}

fn check_registry(ctx: &mut FreshnessContext, policy: &TomlValue) {
    match policy.get("registry").and_then(TomlValue::as_str) {
        Some(registry)
            if registry.starts_with("https://") && !registry.chars().any(char::is_whitespace) =>
        {
            ctx.pass_row("policy-header", "registry", registry);
        }
        Some(registry) => ctx.fail_row(
            "policy-header",
            "registry",
            &format!("must be an https URL, got {registry:?}"),
        ),
        None => ctx.fail_row("policy-header", "registry", "must be an https URL"),
    }
}

/// Compare compiled tool/action pins against the JSON inventory.
pub(crate) fn check_local_pins(ctx: &mut FreshnessContext) {
    pins::check_local_pins(ctx);
}

/// Compare inventory values with the reviewed version-policy mirror.
pub(crate) fn check_policy_mirror(ctx: &mut FreshnessContext) {
    mirror::check_policy_mirror(ctx);
}

/// Parse and compare the three bootstrap tools used by `verify-local.sh`.
pub(crate) fn toolchain_specs(root: &Path) -> Result<Vec<String>, String> {
    let mise = read_toml(root.join("mise.toml"))?;
    let policy = read_toml(root.join(".velnor/version-policy.toml"))?;
    let mise_tools = mise
        .get("tools")
        .and_then(TomlValue::as_table)
        .ok_or("mise.toml [tools] table missing")?;
    let policy_tools = policy
        .get("tools")
        .and_then(TomlValue::as_table)
        .ok_or("version-policy [tools] table missing")?;
    let pairs = [
        ("rust", "rust"),
        ("mr-boxington", "mr-boxington"),
        ("aqua:nextest-rs/nextest/cargo-nextest", "nextest"),
    ];
    pairs.into_iter().map(|(mise_key, policy_key)| {
        let mise_pin = mise_tools.get(mise_key).and_then(TomlValue::as_str).ok_or_else(|| format!("mise.toml[{mise_key}] missing"))?;
        let policy_pin = policy_tools.get(policy_key).and_then(TomlValue::as_str).ok_or_else(|| format!("version-policy[{policy_key}] missing"))?;
        if mise_pin != policy_pin {
            return Err(format!("pin drift: mise.toml[{mise_key}]={mise_pin:?} vs version-policy[{policy_key}]={policy_pin:?}"));
        }
        Ok(format!("{mise_key}@{mise_pin}"))
    }).collect()
}

/// Return the policy's Mise version for the local verification warning.
pub(crate) fn policy_mise_version(root: &Path) -> Result<String, String> {
    let policy = read_toml(root.join(".velnor/version-policy.toml"))?;
    policy
        .get("tools")
        .and_then(|tools| tools.get("mise"))
        .and_then(TomlValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "version-policy [tools].mise missing".to_owned())
}

fn read_toml(path: impl AsRef<Path>) -> Result<TomlValue, String> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .map_err(|error| format!("{} unreadable ({error})", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("{} invalid TOML ({error})", path.display()))
}

fn toml_int(value: &TomlValue, key: &str) -> Option<i64> {
    value.get(key).and_then(TomlValue::as_integer)
}

fn display_json(value: Option<&Value>) -> String {
    value.map_or_else(|| "null".to_owned(), ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{policy_mise_version, toolchain_specs};
    use std::path::Path;

    #[test]
    fn verify_local_toolchain_specs_match_the_policy() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let specs = toolchain_specs(&root);
        assert!(specs.is_ok(), "toolchain specs: {specs:?}");
        assert_eq!(specs.unwrap_or_default().len(), 3);
        assert_eq!(policy_mise_version(&root), Ok("2026.9.18".to_owned()));
    }
}
