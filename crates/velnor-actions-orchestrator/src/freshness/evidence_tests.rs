use std::path::Path;

use serde_json::json;

use super::*;

const DAY: i64 = 86_400_000_000;

fn context_at_utc_end_of_day() -> FreshnessContext {
    let mut ctx = FreshnessContext::new(Path::new("."), false, false);
    ctx.now_micros = super::super::context::days_from_civil(2026, 10, 5) * DAY
        + 23 * 3_600_000_000
        + 59 * 60_000_000;
    ctx
}

#[test]
fn future_date_only_evidence_fails_by_utc_calendar_day() {
    let mut ctx = context_at_utc_end_of_day();
    let result = evidence_timestamp(&mut ctx, "mise", &json!({"checked_at": "2026-10-06"}));
    assert!(result.is_none());
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("is in the future"))
    );
}

#[test]
fn same_day_date_only_evidence_is_not_future() {
    let mut ctx = context_at_utc_end_of_day();
    let result = evidence_timestamp(&mut ctx, "mise", &json!({"checked_at": "2026-10-05"}));
    assert!(result.is_some());
    assert!(ctx.failures.is_empty(), "{:?}", ctx.failures);
}

#[test]
fn timestamp_future_skew_remains_six_minutes() {
    let mut within = context_at_utc_end_of_day();
    let result = evidence_timestamp(
        &mut within,
        "mise",
        &json!({"checked_at": "2026-10-06T00:04:00Z"}),
    );
    assert!(result.is_some());
    assert!(within.failures.is_empty(), "{:?}", within.failures);

    let mut outside = context_at_utc_end_of_day();
    let result = evidence_timestamp(
        &mut outside,
        "mise",
        &json!({"checked_at": "2026-10-06T00:06:01Z"}),
    );
    assert!(result.is_none());
    assert!(
        outside
            .failures
            .iter()
            .any(|row| row.contains("is in the future"))
    );
}
