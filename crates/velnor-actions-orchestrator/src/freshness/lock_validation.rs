//! Validate lockfile field shapes before identity and graph traversal.

use serde_json::Value;

use super::context::{FreshnessContext, py_repr};

const CHECK: &str = "lock-staleness";

pub(super) fn validate(ctx: &mut FreshnessContext, locked: &[Value]) -> bool {
    let mut valid = true;
    for entry in locked {
        if !entry.is_object() {
            valid = false;
            ctx.fail_row(
                CHECK,
                "(lock-package)",
                &format!(
                    "package entry must be an object, got {}",
                    py_repr(Some(entry))
                ),
            );
            continue;
        }
        let subject = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or("(unknown package)")
            .to_owned();
        valid &= validate_required_string(ctx, &subject, entry, "name");
        valid &= validate_required_string(ctx, &subject, entry, "version");
        if let Some(source) = entry.get("source")
            && source.as_str().is_none_or(str::is_empty)
        {
            valid = false;
            ctx.fail_row(
                CHECK,
                &subject,
                &format!(
                    "lock package source must be a non-empty string when present, got {}",
                    py_repr(Some(source))
                ),
            );
        }
        valid &= validate_dependencies(ctx, &subject, entry);
    }
    valid
}

fn validate_required_string(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
    field: &str,
) -> bool {
    let value = entry.get(field);
    if value
        .and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty())
    {
        return true;
    }
    ctx.fail_row(
        CHECK,
        subject,
        &format!(
            "lock package {field} must be a non-empty string, got {}",
            py_repr(value)
        ),
    );
    false
}

fn validate_dependencies(ctx: &mut FreshnessContext, subject: &str, entry: &Value) -> bool {
    let Some(dependencies) = entry.get("dependencies") else {
        return true;
    };
    let Some(dependencies) = dependencies.as_array() else {
        ctx.fail_row(
            CHECK,
            subject,
            &format!(
                "lock package dependencies must be an array, got {}",
                py_repr(Some(dependencies))
            ),
        );
        return false;
    };
    let mut valid = true;
    for (index, dependency) in dependencies.iter().enumerate() {
        if !dependency.is_string() {
            valid = false;
            ctx.fail_row(
                CHECK,
                subject,
                &format!(
                    "lock dependency edge {index} must be a string, got {}",
                    py_repr(Some(dependency))
                ),
            );
        }
    }
    valid
}
