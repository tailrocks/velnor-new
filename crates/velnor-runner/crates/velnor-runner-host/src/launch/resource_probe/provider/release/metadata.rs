//! This module checks text, numbers, and digests in the release manifest.

use serde_json::Value;

pub(super) fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

pub(super) fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

pub(super) fn valid_digest_field(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix("sha256:"))
        .is_some_and(valid_sha256)
}

pub(super) fn valid_hex_field(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(valid_sha256)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
