//! Inventory, local pin, and reviewed policy mirror checks.

use serde_json::{Value, json};

use super::context::{FreshnessContext, parse_timestamp, py_repr};

pub(super) const EXPECTED_TOOLS: [(&str, &str); 10] = [
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

pub(super) const EXPECTED_ACTIONS: [(&str, &str); 9] = [
    ("jdx/mise-action", "MISE_ACTION"),
    ("actions/checkout", "CHECKOUT_ACTION"),
    ("actions/download-artifact", "DOWNLOAD_ARTIFACT_ACTION"),
    ("actions/upload-artifact", "UPLOAD_ARTIFACT_ACTION"),
    ("actions/cache/restore", "CACHE_ACTION"),
    ("actions/cache/save", "CACHE_ACTION"),
    ("jdx/mr-boxington-action", "MR_BOXINGTON_ACTION"),
    ("asamarts/alint", "ALINT_ACTION"),
    ("Swatinem/rust-cache", "RUST_CACHE_ACTION"),
];

pub(super) fn load_inventory(ctx: &mut FreshnessContext) -> bool {
    let bytes = match std::fs::read(&ctx.inv_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            ctx.early_error = Some(format!("unreadable inventory ({error})"));
            return false;
        }
    };
    let inventory = match serde_json::from_slice::<Value>(&bytes) {
        Ok(inventory) => inventory,
        Err(error) => {
            ctx.early_error = Some(format!("unreadable inventory ({error})"));
            return false;
        }
    };
    if !inventory.is_object() {
        ctx.early_error = Some("inventory root must be an object".to_owned());
        return false;
    }
    ctx.inv = inventory;
    check_inventory_shape(ctx);
    true
}

pub(super) fn array_field(ctx: &mut FreshnessContext, field: &str) -> Option<Vec<Value>> {
    match ctx.inv.get(field).cloned() {
        None => Some(Vec::new()),
        Some(Value::Array(values)) => Some(values),
        Some(value) => {
            ctx.fail_row(
                "inventory-shape",
                field,
                &format!("must be an array, got {}", py_repr(Some(&value))),
            );
            None
        }
    }
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
    let mut keys = object_keys(&ctx.inv);
    keys.sort();
    for key in keys {
        if !known.contains(&key.as_str()) {
            ctx.info_row("inventory-shape", &key, "unrecognized top-level key");
        }
    }
    let schema = ctx.inv.get("schema");
    if schema != Some(&json!(1)) {
        ctx.fail_row(
            "inventory-shape",
            "schema",
            &format!("must be 1, got {}", py_repr(schema)),
        );
    }
    let interval = ctx.inv.get("check_interval_hours").cloned();
    ctx.interval = positive_integer(interval.as_ref(), 24, ctx, "check_interval_hours");
    let max_days = ctx.inv.get("max_exception_days").cloned();
    ctx.max_days = positive_integer(max_days.as_ref(), 14, ctx, "max_exception_days");
    ctx.top_checked = ctx.inv.get("checked_at").cloned();
    if ctx
        .top_checked
        .as_ref()
        .is_some_and(|stamp| parse_timestamp(stamp).is_none())
    {
        ctx.fail_row(
            "inventory-shape",
            "checked_at",
            &format!("malformed timestamp {}", py_repr(ctx.top_checked.as_ref())),
        );
    }
}

fn positive_integer(
    value: Option<&Value>,
    default: i64,
    ctx: &mut FreshnessContext,
    field: &str,
) -> i64 {
    if let Some(number) = value.and_then(Value::as_i64).filter(|number| *number > 0) {
        number
    } else {
        ctx.fail_row(
            "inventory-shape",
            field,
            &format!("must be a positive int, got {}", py_repr(value)),
        );
        default
    }
}

pub(super) fn load_policy(ctx: &mut FreshnessContext) {
    let path = ctx.path(".velnor/version-policy.toml");
    let loaded = std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| toml::from_str::<toml::Value>(&text).map_err(|error| error.to_string()))
        .and_then(|policy| serde_json::to_value(policy).map_err(|error| error.to_string()));
    match loaded {
        Ok(policy) => ctx.policy = Some(policy),
        Err(error) => ctx.fail_row(
            "policy-header",
            "version-policy.toml",
            &format!("unreadable ({error})"),
        ),
    }
}

pub(super) fn check_policy_header(ctx: &mut FreshnessContext) {
    let Some(policy) = ctx.policy.clone() else {
        return;
    };
    let allowed_tables = [
        "tools",
        "github_runner_images",
        "actions",
        "validation-tools",
    ];
    let allowed_scalars = [
        "schema",
        "channel",
        "registry",
        "check_interval_hours",
        "max_exception_days",
    ];
    let mut keys = object_keys(&policy);
    keys.sort();
    for key in keys {
        if !allowed_tables.contains(&key.as_str()) && !allowed_scalars.contains(&key.as_str()) {
            ctx.fail_row("policy-header", &key, "unknown key rejected");
        }
    }
    check_header_value(ctx, &policy, "schema", &json!(1));
    check_header_value(ctx, &policy, "channel", &json!("stable"));
    check_registry(ctx, policy.get("registry"));
    check_header_value(ctx, &policy, "check_interval_hours", &json!(ctx.interval));
    check_header_value(ctx, &policy, "max_exception_days", &json!(ctx.max_days));
    for (key, cap) in [("check_interval_hours", 24), ("max_exception_days", 14)] {
        if policy
            .get(key)
            .and_then(Value::as_i64)
            .is_some_and(|number| number > cap)
        {
            ctx.fail_row(
                "policy-header",
                key,
                &format!("weakens policy: must be <= {cap}"),
            );
        }
    }
}

fn check_header_value(ctx: &mut FreshnessContext, policy: &Value, key: &str, want: &Value) {
    let got = policy.get(key);
    if got == Some(want) {
        ctx.pass_row("policy-header", key, &py_repr(got));
    } else {
        ctx.fail_row(
            "policy-header",
            key,
            &format!("got={} want={}", py_repr(got), py_repr(Some(want))),
        );
    }
}

fn check_registry(ctx: &mut FreshnessContext, value: Option<&Value>) {
    let Some(registry) = value.and_then(Value::as_str) else {
        ctx.fail_row(
            "policy-header",
            "registry",
            &format!("must be an https URL, got {}", py_repr(value)),
        );
        return;
    };
    if !registry.starts_with("https://") || registry.chars().any(char::is_whitespace) {
        ctx.fail_row(
            "policy-header",
            "registry",
            &format!("must be an https URL, got {}", py_repr(value)),
        );
    } else {
        ctx.pass_row("policy-header", "registry", registry);
    }
}

fn object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}
