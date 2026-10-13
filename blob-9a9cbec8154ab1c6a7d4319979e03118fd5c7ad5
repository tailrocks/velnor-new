use serde_json::json;

use super::{freshness_row, future_evidence};
use crate::context::{FreshnessContext, parse_timestamp};

#[test]
fn future_timestamps_apply_date_and_clock_rules() {
    let now = 1_799_999_000_i64;
    assert!(!future_evidence(now, "2026-10-05", now));
    assert!(future_evidence(now, "2099-01-01", 4_070_908_800));
    assert!(future_evidence(now, "2099-01-01T00:00:00Z", 4_070_908_800));
}

#[test]
fn precise_evidence_keeps_its_24_hour_window_across_midnight() {
    let now = parse_timestamp("2026-10-09T00:00:01Z").expect("fixed timestamp is valid");
    let pinned = json!("1.2.3");
    let mut boundary = FreshnessContext::new(std::path::PathBuf::new(), false, false);
    boundary.now = now;
    let recent = json!({
        "source": "https://example.test/releases",
        "status": "current",
        "checked_at": "2026-10-08T23:59:59Z"
    });
    freshness_row(
        &mut boundary,
        "tool",
        &recent,
        Some(&pinned),
        Some(&pinned),
        Some(&pinned),
        None,
    );
    assert!(boundary.failures.is_empty(), "{:?}", boundary.failures);

    let mut midnight = FreshnessContext::new(std::path::PathBuf::new(), false, false);
    midnight.now = now;
    let date_only = json!({
        "source": "https://example.test/releases",
        "status": "current",
        "checked_at": "2026-10-08"
    });
    freshness_row(
        &mut midnight,
        "tool",
        &date_only,
        Some(&pinned),
        Some(&pinned),
        Some(&pinned),
        None,
    );
    assert!(
        midnight
            .failures
            .iter()
            .any(|failure| failure.contains("stale evidence")),
        "{:?}",
        midnight.failures
    );

    let mut aged = FreshnessContext::new(std::path::PathBuf::new(), false, false);
    aged.now = now;
    let stale = json!({
        "source": "https://example.test/releases",
        "status": "current",
        "checked_at": "2026-10-07T23:59:59Z"
    });
    freshness_row(
        &mut aged,
        "tool",
        &stale,
        Some(&pinned),
        Some(&pinned),
        Some(&pinned),
        None,
    );
    assert!(
        aged.failures
            .iter()
            .any(|failure| failure.contains("stale evidence")),
        "{:?}",
        aged.failures
    );
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
