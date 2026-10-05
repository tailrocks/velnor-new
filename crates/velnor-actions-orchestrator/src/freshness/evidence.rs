//! Recorded upstream evidence freshness and optional latest-value checks.

use serde_json::{Value, json};

use super::context::{
    FreshnessContext, current_date, norm_version, parse_iso_date, parse_timestamp, py_repr,
};
use super::inventory::array_field;

pub(super) fn check_recorded_freshness(ctx: &mut FreshnessContext) {
    ctx.holds = array_field(ctx, "temporary_holds").unwrap_or_default();
    ctx.hold_keys = ctx
        .holds
        .iter()
        .filter_map(|hold| hold.get("key").and_then(Value::as_str).map(str::to_owned))
        .collect();
    for tool in ctx.tools.clone() {
        freshness_row(
            ctx,
            tool.get("name"),
            &tool,
            tool.get("pinned"),
            tool.get("qualified"),
            tool.get("latest"),
            None,
        );
    }
    let actions: Vec<(String, Value)> = ctx
        .action_pinned
        .iter()
        .map(|(key, action)| (key.clone(), action.clone()))
        .collect();
    for (key, action) in actions {
        let subject = Value::String(key.clone());
        let pinned = json!([action.get("pinned_version"), action.get("pinned_sha")]);
        let qualified = json!([action.get("qualified_version"), action.get("qualified_sha")]);
        freshness_row(
            ctx,
            Some(&subject),
            &action,
            Some(&pinned),
            Some(&qualified),
            action.get("latest"),
            action.get("pinned_version"),
        );
    }
    let runner = ctx.runner.clone();
    let runner_pin = runner.get("default");
    freshness_row(
        ctx,
        Some(&Value::String("runner".to_owned())),
        &runner,
        runner_pin,
        runner_pin,
        runner.get("latest"),
        None,
    );
}

const MICROS_PER_HOUR: i64 = 3_600_000_000;

fn freshness_row(
    ctx: &mut FreshnessContext,
    subject: Option<&Value>,
    entry: &Value,
    pinned: Option<&Value>,
    qualified: Option<&Value>,
    latest: Option<&Value>,
    pin_for_latest: Option<&Value>,
) {
    let subject = subject
        .and_then(Value::as_str)
        .map_or_else(|| py_repr(subject), str::to_owned);
    let Some(source) = evidence_source(ctx, &subject, entry) else {
        return;
    };
    let Some((stamp, stamp_micros)) = evidence_timestamp(ctx, &subject, entry) else {
        return;
    };
    if held_row(ctx, &subject, entry, &stamp) {
        return;
    }
    if !current_status(ctx, &subject, entry, source, &stamp)
        || !within_interval(ctx, &subject, &stamp, stamp_micros)
    {
        return;
    }
    if pinned != qualified {
        ctx.fail_row(
            "upstream-freshness",
            &subject,
            &format!(
                "unqualified pin: pinned={} qualified={}",
                tuple_repr(pinned),
                tuple_repr(qualified)
            ),
        );
        return;
    }
    check_latest(
        ctx,
        &subject,
        source,
        &stamp,
        pinned,
        latest,
        pin_for_latest,
    );
}

fn evidence_source<'a>(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &'a Value,
) -> Option<&'a str> {
    let source = entry.get("source").and_then(Value::as_str);
    let Some(source) = source.filter(|source| source.contains("://")) else {
        ctx.fail_row("upstream-freshness", subject, "missing source URL");
        return None;
    };
    if ["https://", "http://", "file://"]
        .iter()
        .any(|prefix| source.starts_with(prefix))
    {
        Some(source)
    } else {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "unsupported source scheme: {}",
                py_repr(Some(&json!(source)))
            ),
        );
        None
    }
}

fn evidence_timestamp(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
) -> Option<(String, i64)> {
    let value = entry
        .get("checked_at")
        .cloned()
        .or_else(|| ctx.top_checked.clone());
    let Some(stamp_micros) = value.as_ref().and_then(parse_timestamp) else {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "missing or malformed check timestamp {}",
                py_repr(value.as_ref())
            ),
        );
        return None;
    };
    let stamp = value
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let trimmed = stamp.trim();
    let date_only = trimmed.len() == 10
        && trimmed.as_bytes().get(4) == Some(&b'-')
        && trimmed.as_bytes().get(7) == Some(&b'-')
        && !trimmed.contains(['T', ':']);
    let future = if date_only {
        parse_iso_date(&json!(trimmed)).is_some_and(|date| date > current_date(ctx.now_micros))
    } else {
        ctx.now_micros.saturating_sub(stamp_micros) < -360_000_000
    };
    if future {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!("check timestamp {stamp} is in the future"),
        );
        None
    } else {
        Some((stamp, stamp_micros))
    }
}

fn current_status(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
    source: &str,
    stamp: &str,
) -> bool {
    if entry.get("status").and_then(Value::as_str) == Some("current") {
        return true;
    }
    ctx.fail_row(
        "upstream-freshness",
        subject,
        &format!(
            "status={}: refresh required, never current (source {source}, checked {stamp})",
            py_repr(entry.get("status"))
        ),
    );
    false
}

fn within_interval(
    ctx: &mut FreshnessContext,
    subject: &str,
    stamp: &str,
    stamp_micros: i64,
) -> bool {
    let age_micros = ctx.now_micros.saturating_sub(stamp_micros);
    if age_micros <= ctx.interval.saturating_mul(MICROS_PER_HOUR) {
        return true;
    }
    let tenths = rounded_tenths_of_hour(age_micros);
    ctx.fail_row(
        "upstream-freshness",
        subject,
        &format!(
            "stale evidence: checked {stamp} ({}.{:01}h ago, interval {}h)",
            tenths / 10,
            tenths % 10,
            ctx.interval
        ),
    );
    false
}

fn rounded_tenths_of_hour(micros: i64) -> u64 {
    let micros = micros.unsigned_abs();
    let divisor = u64::try_from(MICROS_PER_HOUR / 10).unwrap_or(1);
    let quotient = micros / divisor;
    let remainder = micros % divisor;
    if remainder * 2 > divisor || (remainder * 2 == divisor && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    }
}

fn held_row(ctx: &mut FreshnessContext, subject: &str, entry: &Value, stamp: &str) -> bool {
    if entry.get("status").and_then(Value::as_str) != Some("held") {
        return false;
    }
    let entry_key = entry.get("key").and_then(Value::as_str).unwrap_or(subject);
    if !ctx.hold_keys.contains(subject) && !ctx.hold_keys.contains(entry_key) {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            "status=held without a covering temporary hold",
        );
    } else {
        ctx.pass_row(
            "upstream-freshness",
            subject,
            &format!("held, evidence {stamp}"),
        );
    }
    true
}

fn check_latest(
    ctx: &mut FreshnessContext,
    subject: &str,
    source: &str,
    stamp: &str,
    pinned: Option<&Value>,
    latest: Option<&Value>,
    pin_for_latest: Option<&Value>,
) {
    let compared_pin = pin_for_latest.or(pinned);
    if latest.is_some() && norm_version(latest) != norm_version(compared_pin) {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "stale pin: pinned={} latest={} (source {source}, checked {stamp})",
                tuple_repr(pinned),
                tuple_repr(latest)
            ),
        );
    } else {
        ctx.pass_row(
            "upstream-freshness",
            subject,
            &format!("current, evidence {stamp}"),
        );
    }
}

fn tuple_repr(value: Option<&Value>) -> String {
    match value {
        Some(Value::Array(values)) => format!(
            "({}{})",
            values
                .iter()
                .map(|value| py_repr(Some(value)))
                .collect::<Vec<_>>()
                .join(", "),
            if values.len() == 1 { "," } else { "" }
        ),
        _ => py_repr(value),
    }
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
