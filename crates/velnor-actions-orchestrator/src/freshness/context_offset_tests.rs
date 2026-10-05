use super::context::parse_timestamp;
use serde_json::json;

#[test]
fn malformed_four_byte_non_ascii_offset_fails_without_panicking() {
    assert!(parse_timestamp(&json!("2026-10-05T12:34:56+€x")).is_none());
}

#[test]
fn numeric_offsets_and_date_only_timestamps_remain_valid() {
    assert!(parse_timestamp(&json!("2026-10-05T12:34:56+01:30")).is_some());
    assert!(parse_timestamp(&json!("2026-10-05T12:34:56+0130")).is_some());
    assert!(parse_timestamp(&json!("2026-10-05")).is_some());
}
