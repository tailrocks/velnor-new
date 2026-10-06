//! Freshness records for pinned tools, actions, and runners.

use serde_json::{Value, json};

use crate::context::{FreshnessContext, norm_version, parse_timestamp};

use super::display;

pub(crate) fn check_recorded_freshness(ctx: &mut FreshnessContext) {
    ctx.holds = ctx
        .inv
        .get("temporary_holds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    ctx.hold_keys = ctx
        .holds
        .iter()
        .filter_map(|hold| hold.get("key").and_then(Value::as_str).map(str::to_owned))
        .collect();
    for tool in ctx.tools.clone() {
        let subject = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<missing>")
            .to_owned();
        freshness_row(
            ctx,
            &subject,
            &tool,
            tool.get("pinned"),
            tool.get("qualified"),
            tool.get("latest"),
            None,
        );
    }
    let actions = ctx
        .action_pinned
        .iter()
        .map(|(key, action)| (key.clone(), action.clone()))
        .collect::<Vec<_>>();
    for (key, action) in actions {
        let pinned = json!([action.get("pinned_version"), action.get("pinned_sha")]);
        let qualified = json!([action.get("qualified_version"), action.get("qualified_sha")]);
        freshness_row(
            ctx,
            &key,
            &action,
            Some(&pinned),
            Some(&qualified),
            action.get("latest"),
            action.get("pinned_version"),
        );
    }
    let runner = ctx.runner.clone();
    let default = runner.get("default");
    freshness_row(ctx, "runner", &runner, default, default, None, None);
}

fn freshness_row(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
    pinned: Option<&Value>,
    qualified: Option<&Value>,
    latest: Option<&Value>,
    pin_for_latest: Option<&Value>,
) {
    let Some(source) = source_url(ctx, subject, entry) else {
        return;
    };
    let Some((stamp, moment)) = evidence_time(ctx, subject, entry) else {
        return;
    };
    if held_row(ctx, subject, entry, &stamp, pin_for_latest.or(pinned)) {
        return;
    }
    if entry.get("status").and_then(Value::as_str) != Some("current") {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "status={}: refresh required, never current (source {source}, checked {stamp})",
                display(entry.get("status"))
            ),
        );
        return;
    }
    if !fresh_within_interval(ctx, subject, &stamp, moment) {
        return;
    }
    if pinned != qualified {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "unqualified pin: pinned={} qualified={}",
                display(pinned),
                display(qualified)
            ),
        );
        return;
    }
    check_latest(
        ctx,
        subject,
        &source,
        &stamp,
        pinned,
        latest,
        pin_for_latest,
    );
}

fn source_url(ctx: &mut FreshnessContext, subject: &str, entry: &Value) -> Option<String> {
    let source = entry.get("source").and_then(Value::as_str).unwrap_or("");
    if !source.contains("://") {
        ctx.fail_row("upstream-freshness", subject, "missing source URL");
        return None;
    }
    if !["https://", "http://", "file://"]
        .iter()
        .any(|scheme| source.starts_with(scheme))
    {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!("unsupported source scheme: {source:?}"),
        );
        return None;
    }
    Some(source.to_owned())
}

fn evidence_time(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
) -> Option<(String, i64)> {
    let stamp = match entry.get("checked_at") {
        Some(Value::String(stamp)) => Some(stamp.clone()),
        Some(value) => {
            ctx.fail_row(
                "upstream-freshness",
                subject,
                &format!("checked_at must be a string, got {value}"),
            );
            return None;
        }
        None => ctx.top_checked.clone(),
    };
    let Some(stamp) = stamp else {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            "missing or malformed check timestamp null",
        );
        return None;
    };
    let Some(moment) = parse_timestamp(&stamp) else {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!("missing or malformed check timestamp {stamp:?}"),
        );
        return None;
    };
    if future_evidence(ctx.now, &stamp, moment) {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!("check timestamp {stamp} is in the future"),
        );
        return None;
    }
    Some((stamp, moment))
}

fn fresh_within_interval(
    ctx: &mut FreshnessContext,
    subject: &str,
    stamp: &str,
    moment: i64,
) -> bool {
    let age = ctx.now.saturating_sub(moment);
    let interval_seconds = ctx.interval.saturating_mul(3_600);
    if age > interval_seconds {
        let tenths = age.saturating_add(180) / 360;
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
    } else {
        true
    }
}

fn future_evidence(now: i64, stamp: &str, moment: i64) -> bool {
    if stamp.len() == 10 && !stamp.chars().any(|ch| matches!(ch, 'T' | 't' | ':')) {
        let today = now.div_euclid(86_400);
        return moment.div_euclid(86_400) > today;
    }
    moment.saturating_sub(now) > 360
}

fn held_row(
    ctx: &mut FreshnessContext,
    subject: &str,
    entry: &Value,
    stamp: &str,
    pin: Option<&Value>,
) -> bool {
    if entry.get("status").and_then(Value::as_str) != Some("held") {
        return false;
    }
    let key = entry.get("key").and_then(Value::as_str).unwrap_or(subject);
    if !ctx.hold_keys.contains(subject) && !ctx.hold_keys.contains(key) {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            "status=held without a covering temporary hold",
        );
        return true;
    }
    let version = pin.and_then(Value::as_str).unwrap_or("");
    let covering_hold = ctx.holds.iter().find(|hold| {
        let hold_key = hold.get("key").and_then(Value::as_str);
        hold_key.is_some_and(|hold_key| hold_key == subject || hold_key == key)
    });
    let Some(hold) = covering_hold else {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            "status=held without a covering temporary hold",
        );
        return true;
    };
    let held_version = hold.get("held_version").and_then(Value::as_str);
    if held_version != Some(version) {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "temporary hold covers held_version={}, but pinned version is {version:?}",
                display(hold.get("held_version"))
            ),
        );
        return true;
    }
    ctx.pass_row(
        "upstream-freshness",
        subject,
        &format!("held, evidence {stamp}"),
    );
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
    let compared = pin_for_latest
        .or(pinned)
        .and_then(Value::as_str)
        .unwrap_or("");
    let latest = latest.and_then(Value::as_str);
    if latest.is_some_and(|latest| norm_version(latest) != norm_version(compared)) {
        ctx.fail_row(
            "upstream-freshness",
            subject,
            &format!(
                "stale pin: pinned={} latest={latest:?} (source {source}, checked {stamp})",
                display(pinned)
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{freshness_row, future_evidence};
    use crate::context::FreshnessContext;

    #[test]
    fn future_timestamps_apply_date_and_clock_rules() {
        let now = 1_799_999_000_i64;
        assert!(!future_evidence(now, "2026-10-05", now));
        assert!(future_evidence(now, "2099-01-01", 4_070_908_800));
        assert!(future_evidence(now, "2099-01-01T00:00:00Z", 4_070_908_800));
    }

    #[test]
    fn held_status_requires_an_exact_pin_version_match() {
        let mut context = FreshnessContext::new(std::path::PathBuf::new(), false, false);
        context.holds = vec![json!({"key": "cli/cli", "held_version": "9.9.9"})];
        context.hold_keys.insert("cli/cli".to_owned());
        let entry = json!({
            "key": "cli/cli",
            "status": "held",
            "source": "https://example.test/releases",
            "checked_at": "2026-10-05T00:00:00Z"
        });
        let wrong_pin = json!("2.101.0");
        let matching_pin = json!("9.9.9");

        freshness_row(
            &mut context,
            "cli/cli",
            &entry,
            Some(&wrong_pin),
            Some(&wrong_pin),
            Some(&wrong_pin),
            None,
        );
        assert!(context.failures.iter().any(|failure| {
            failure.contains("held_version=\"9.9.9\"")
                && failure.contains("pinned version is \"2.101.0\"")
        }));

        let mut matching_context = FreshnessContext::new(std::path::PathBuf::new(), false, false);
        matching_context.holds = context.holds.clone();
        matching_context.hold_keys = context.hold_keys.clone();
        freshness_row(
            &mut matching_context,
            "cli/cli",
            &entry,
            Some(&matching_pin),
            Some(&matching_pin),
            Some(&matching_pin),
            None,
        );
        assert!(matching_context.failures.is_empty());

        let mut action_context = FreshnessContext::new(std::path::PathBuf::new(), false, false);
        action_context.holds = vec![json!({
            "key": "jdx/mr-boxington-action",
            "held_version": "v1.6.0"
        })];
        action_context
            .hold_keys
            .insert("jdx/mr-boxington-action".to_owned());
        let action_entry = json!({
            "key": "jdx/mr-boxington-action",
            "status": "held",
            "source": "https://example.test/releases",
            "checked_at": "2026-10-05T00:00:00Z"
        });
        let composite_pin = json!(["v1.6.0", "a".repeat(40)]);
        let version_pin = json!("v1.6.0");
        freshness_row(
            &mut action_context,
            "jdx/mr-boxington-action",
            &action_entry,
            Some(&composite_pin),
            Some(&composite_pin),
            Some(&version_pin),
            Some(&version_pin),
        );
        assert!(action_context.failures.is_empty());
    }
}
