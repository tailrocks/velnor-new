//! Dated and standing freshness exceptions.

use std::collections::BTreeSet;

use serde_json::Value;

use super::context::{
    CivilDate, FreshnessContext, current_date, days_from_civil, parse_iso_date, py_repr,
};
use super::inventory::{EXPECTED_ACTIONS, EXPECTED_TOOLS, array_field};

const BLESSED_STANDING: &str = "asamarts/alint";

pub(super) fn check_exceptions(ctx: &mut FreshnessContext) {
    let today = current_date(ctx.now_micros);
    let known_subjects = known_subjects(ctx);
    let holds = ctx.holds.clone();
    for hold in &holds {
        check_dated(ctx, hold, "exception-expiry", today, &known_subjects);
    }
    if holds.is_empty() {
        ctx.pass_row("exception-expiry", "(none)", "no temporary holds");
    }
    let reviewed_alint = ctx
        .action_pinned
        .get(BLESSED_STANDING)
        .and_then(|action| action.get("pinned_version"))
        .cloned();
    let exceptions = array_field(ctx, "exceptions").unwrap_or_default();
    for entry in exceptions {
        check_standing_record(ctx, &entry, today, &known_subjects, reviewed_alint.as_ref());
    }
}

fn known_subjects(ctx: &FreshnessContext) -> BTreeSet<String> {
    let lock_names = ctx
        .locked
        .as_ref()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .map(str::to_owned);
    let tools = EXPECTED_TOOLS.into_iter().map(|(name, _)| name.to_owned());
    let actions = EXPECTED_ACTIONS
        .into_iter()
        .map(|(name, _)| name.to_owned());
    let labels = ctx
        .supported
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned);
    tools
        .chain(actions)
        .chain(lock_names)
        .chain(labels)
        .chain(
            ctx.runner
                .get("default")
                .and_then(Value::as_str)
                .map(str::to_owned),
        )
        .collect()
}

fn check_dated(
    ctx: &mut FreshnessContext,
    entry: &Value,
    check: &str,
    today: CivilDate,
    known_subjects: &BTreeSet<String>,
) {
    if !entry.is_object() {
        ctx.fail_row(
            check,
            "(inventory exceptions)",
            &format!("entry must be an object, got {}", py_repr(Some(entry))),
        );
        return;
    }
    let subject = entry
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed hold>");
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
        .filter(|field| !is_truthy(entry.get(**field)))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        ctx.fail_row(check, subject, &format!("missing {}", missing.join(",")));
        return;
    }
    let granted_value = entry.get("granted");
    let expires_value = entry.get("expires");
    let granted = granted_value.and_then(parse_iso_date);
    let expires = expires_value.and_then(parse_iso_date);
    let (Some(granted), Some(expires)) = (granted, expires) else {
        ctx.fail_row(check, subject, "granted/expires must be YYYY-MM-DD");
        return;
    };
    if granted > today {
        ctx.fail_row(
            check,
            subject,
            &format!("granted {granted} is in the future"),
        );
    } else if expires <= granted {
        ctx.fail_row(
            check,
            subject,
            &format!("inverted window: expires {expires} <= granted {granted}"),
        );
    } else {
        check_dated_bounds(ctx, subject, check, granted, expires, today, known_subjects);
    }
}

fn check_dated_bounds(
    ctx: &mut FreshnessContext,
    subject: &str,
    check: &str,
    granted: CivilDate,
    expires: CivilDate,
    today: CivilDate,
    known_subjects: &BTreeSet<String>,
) {
    let span = days_from_civil(expires.year, expires.month, expires.day)
        - days_from_civil(granted.year, granted.month, granted.day);
    if span > ctx.max_days {
        ctx.fail_row(
            check,
            subject,
            &format!("span {span}d exceeds max {}d", ctx.max_days),
        );
    } else if expires < today {
        ctx.fail_row(
            check,
            subject,
            &format!("expired {expires} (renewal needs new review + evidence)"),
        );
    } else if !known_subjects.contains(subject) {
        ctx.fail_row(
            check,
            subject,
            "hold subject matches no inventoried tool, action, runner label, or locked package",
        );
    } else {
        ctx.pass_row(check, subject, &format!("expires {expires}"));
    }
}

fn check_standing_record(
    ctx: &mut FreshnessContext,
    entry: &Value,
    today: CivilDate,
    known_subjects: &BTreeSet<String>,
    reviewed_alint: Option<&Value>,
) {
    if !entry.is_object() {
        ctx.fail_row(
            "standing-exception",
            "(inventory exceptions)",
            &format!("entry must be an object, got {}", py_repr(Some(entry))),
        );
        return;
    }
    let subject = entry
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed hold>");
    if entry.get("expires").is_some_and(|value| !value.is_null()) {
        check_dated(ctx, entry, "standing-exception", today, known_subjects);
    } else if subject != BLESSED_STANDING {
        ctx.fail_row(
            "standing-exception",
            subject,
            "standing hold without a spec blessing (only asamarts/alint is blessed)",
        );
    } else {
        check_blessed_standing(ctx, subject, entry, reviewed_alint);
    }
}

fn check_blessed_standing(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
    reviewed_alint: Option<&Value>,
) {
    let required = ["kind", "expiry_policy", "blessed_by", "tag"];
    let missing = required
        .iter()
        .filter(|field| !is_truthy(entry.get(**field)))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        ctx.fail_row(
            "standing-exception",
            subject,
            &format!("blessed standing exception lacks {}", missing.join(",")),
        );
    } else if entry.get("tag") != reviewed_alint {
        ctx.fail_row(
            "standing-exception",
            subject,
            &format!(
                "blessed tag {} != reviewed pin {}: re-bless on pin moves",
                py_repr(entry.get("tag")),
                py_repr(reviewed_alint)
            ),
        );
    } else {
        ctx.pass_row(
            "standing-exception",
            subject,
            &format!(
                "blessed mutable tag {}",
                entry["tag"].as_str().unwrap_or_default()
            ),
        );
    }
}

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{check_dated, check_standing_record};
    use crate::freshness::context::{CivilDate, FreshnessContext};

    #[test]
    fn dated_hold_enforces_maximum_span_and_known_subject() {
        let mut ctx = FreshnessContext::new(Path::new("."), false, false);
        ctx.max_days = 14;
        let today = CivilDate {
            year: 2026,
            month: 10,
            day: 5,
        };
        let known = ["rust".to_owned()].into_iter().collect();
        let hold = json!({
            "key": "rust", "held_version": "1.98.1", "owner": "team",
            "issue": "#1", "reason": "review", "granted": "2026-10-01",
            "expires": "2026-10-16"
        });
        check_dated(&mut ctx, &hold, "exception-expiry", today, &known);
        assert_eq!(ctx.failures.len(), 1);
        assert!(ctx.failures[0].contains("span 15d exceeds max 14d"));
    }

    #[test]
    fn standing_exception_requires_the_reviewed_mutable_tag() {
        let mut ctx = FreshnessContext::new(Path::new("."), false, false);
        let today = CivilDate {
            year: 2026,
            month: 10,
            day: 5,
        };
        let entry = json!({
            "key": "asamarts/alint", "kind": "mutable-tag",
            "expiry_policy": "reviewed", "blessed_by": "spec", "tag": "v0.1"
        });
        check_standing_record(
            &mut ctx,
            &entry,
            today,
            &std::collections::BTreeSet::default(),
            Some(&json!("v0.2")),
        );
        assert_eq!(ctx.failures.len(), 1);
        assert!(ctx.failures[0].contains("re-bless on pin moves"));
    }
}
