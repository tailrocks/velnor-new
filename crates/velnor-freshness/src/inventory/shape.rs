// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::context::FreshnessContext;

pub(super) fn check_inventory_shape(ctx: &mut FreshnessContext) {
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
        let got = super::display_json(ctx.inv.get("schema"));
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
            let got = super::display_json(ctx.inv.get(key));
            ctx.fail_row(
                "inventory-shape",
                key,
                &format!("must be a positive int, got {got}"),
            );
            default
        }
    }
}
