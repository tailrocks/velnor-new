//! Expiring exceptions and temporary holds.

use serde_json::Value;

use toml::Value as TomlValue;

use crate::context::{FreshnessContext, iso_date, parse_iso_date};

use crate::inventory::{EXPECTED_ACTIONS, EXPECTED_TOOLS};

use super::{BLESSED_STANDING, display};

pub(crate) fn check_exceptions(ctx: &mut FreshnessContext) {
    let today = ctx.now.div_euclid(86_400);
    let known = known_subjects(ctx);
    for hold in ctx.holds.clone() {
        check_dated(ctx, &hold, "exception-expiry", today, &known);
    }
    if ctx.holds.is_empty() {
        ctx.pass_row("exception-expiry", "(none)", "no temporary holds");
    }
    let alint_pin = ctx
        .action_pinned
        .get(BLESSED_STANDING)
        .and_then(|entry| entry.get("pinned_version"))
        .cloned();
    let exceptions = ctx
        .inv
        .get("exceptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for exception in exceptions {
        check_standing(ctx, &exception, alint_pin.as_ref(), today, &known);
    }
}

fn known_subjects(ctx: &FreshnessContext) -> std::collections::BTreeSet<String> {
    let mut subjects = EXPECTED_TOOLS
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .chain(EXPECTED_ACTIONS.iter().map(|(name, _)| (*name).to_owned()))
        .collect::<std::collections::BTreeSet<_>>();
    subjects.extend(ctx.locked.iter().filter_map(|entry| {
        entry
            .get("name")
            .and_then(TomlValue::as_str)
            .map(str::to_owned)
    }));
    subjects.extend(ctx.supported.iter().cloned());
    if let Some(default) = ctx.runner.get("default").and_then(Value::as_str) {
        subjects.insert(default.to_owned());
    }
    subjects
}

fn check_dated(
    ctx: &mut FreshnessContext,
    entry: &Value,
    check: &str,
    today: i64,
    known: &std::collections::BTreeSet<String>,
) {
    let Some(entry) = entry.as_object() else {
        ctx.fail_row(
            check,
            "(inventory exceptions)",
            &format!("entry must be an object, got {entry}"),
        );
        return;
    };
    let subject = entry
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed hold>")
        .to_owned();
    let required = [
        "held_version",
        "owner",
        "issue",
        "reason",
        "granted",
        "expires",
    ];
    let missing = required
        .iter()
        .filter(|key| {
            entry
                .get(**key)
                .is_none_or(|value| value.as_str().is_none_or(str::is_empty))
        })
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        ctx.fail_row(check, &subject, &format!("missing {}", missing.join(",")));
        return;
    }
    let granted_text = entry.get("granted").and_then(Value::as_str).unwrap_or("");
    let expires_text = entry.get("expires").and_then(Value::as_str).unwrap_or("");
    let (Some(granted), Some(expires)) =
        (parse_iso_date(granted_text), parse_iso_date(expires_text))
    else {
        ctx.fail_row(check, &subject, "granted/expires must be YYYY-MM-DD");
        return;
    };
    if !check_date_window(ctx, &subject, check, granted, expires, today) {
        return;
    }
    if known.contains(&subject) {
        ctx.pass_row(check, &subject, &format!("expires {}", iso_date(expires)));
    } else {
        ctx.fail_row(
            check,
            &subject,
            "hold subject matches no inventoried tool, action, runner label, or locked package",
        );
    }
}

fn check_date_window(
    ctx: &mut FreshnessContext,
    subject: &str,
    check: &str,
    granted: i64,
    expires: i64,
    today: i64,
) -> bool {
    if granted > today {
        ctx.fail_row(
            check,
            subject,
            &format!("granted {} is in the future", iso_date(granted)),
        );
        false
    } else if expires <= granted {
        ctx.fail_row(
            check,
            subject,
            &format!(
                "inverted window: expires {} <= granted {}",
                iso_date(expires),
                iso_date(granted)
            ),
        );
        false
    } else if expires - granted > ctx.max_days {
        ctx.fail_row(
            check,
            subject,
            &format!("span {}d exceeds max {}d", expires - granted, ctx.max_days),
        );
        false
    } else if expires < today {
        ctx.fail_row(
            check,
            subject,
            &format!(
                "expired {} (renewal needs new review + evidence)",
                iso_date(expires)
            ),
        );
        false
    } else {
        true
    }
}

fn check_standing(
    ctx: &mut FreshnessContext,
    entry: &Value,
    alint_pin: Option<&Value>,
    today: i64,
    known: &std::collections::BTreeSet<String>,
) {
    let Some(entry) = entry.as_object() else {
        ctx.fail_row(
            "standing-exception",
            "(inventory exceptions)",
            &format!("entry must be an object, got {entry}"),
        );
        return;
    };
    let subject = entry
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed hold>")
        .to_owned();
    if !entry.get("expires").is_none_or(Value::is_null) {
        check_dated(
            ctx,
            &Value::Object(entry.clone()),
            "standing-exception",
            today,
            known,
        );
    } else if subject != BLESSED_STANDING {
        ctx.fail_row(
            "standing-exception",
            &subject,
            "standing hold without a spec blessing (only asamarts/alint is blessed)",
        );
    } else {
        check_blessed_standing(ctx, &subject, entry, alint_pin);
    }
}

fn check_blessed_standing(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &serde_json::Map<String, Value>,
    alint_pin: Option<&Value>,
) {
    let required = ["kind", "expiry_policy", "blessed_by", "tag"];
    let missing = required
        .iter()
        .filter(|key| {
            entry
                .get(**key)
                .is_none_or(|value| value.as_str().is_none_or(str::is_empty))
        })
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        ctx.fail_row(
            "standing-exception",
            subject,
            &format!("blessed standing exception lacks {}", missing.join(",")),
        );
    } else if entry.get("tag") != alint_pin {
        ctx.fail_row(
            "standing-exception",
            subject,
            &format!(
                "blessed tag {} != reviewed pin {}: re-bless on pin moves",
                display(entry.get("tag")),
                display(alint_pin)
            ),
        );
    } else {
        ctx.pass_row(
            "standing-exception",
            subject,
            &format!("blessed mutable tag {}", display(entry.get("tag"))),
        );
    }
}
