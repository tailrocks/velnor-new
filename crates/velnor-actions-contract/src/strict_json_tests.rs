//! Strict-JSON tests: size, shape, and nesting-budget gates.
//!
//! Declared via `#[path]` from `strict_json.rs` under `cfg(test)`.

use std::fmt::Write as _;

use super::{
    MAX_JSON_NESTING_DEPTH, check_document_size, parse_strict_json, parse_strict_json_bytes,
    parse_strict_json_with_limit,
};
use crate::errors::ContractError;

#[test]
fn default_bound_accepts_small_docs() {
    let value = parse_strict_json(r#"{"a":1}"#).expect("small doc");
    assert_eq!(value.get("a").and_then(serde_json::Value::as_u64), Some(1));
}

#[test]
fn oversize_doc_fails_with_size_detail() {
    let big = format!(r#"{{"pad":"{}"}}"#, "x".repeat(100));
    let err = parse_strict_json_with_limit(&big, 16).expect_err("oversize");
    assert!(matches!(
        err,
        ContractError::DocumentTooLarge { size, limit: 16 } if size == big.len()
    ));
    assert!(parse_strict_json_with_limit(&big, big.len()).is_ok());
    assert!(check_document_size(3, 3).is_ok());
    assert!(check_document_size(4, 3).is_err());
}

#[test]
fn bytes_entry_rejects_bad_utf8_and_dup_keys() {
    assert!(parse_strict_json_bytes(b"\xff", 64).is_err());
    let dup = br#"{"a":1,"a":2}"#;
    assert!(parse_strict_json_bytes(dup, 64).is_err());
    assert!(parse_strict_json_bytes(br#"{"a":1}"#, 4).is_err());
    assert!(parse_strict_json_bytes(br#"{"a":1}"#, 64).is_ok());
}

/// Nest `depth` arrays around a scalar.
fn nested_arrays(depth: usize) -> String {
    format!("{}1{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn nesting_boundary_matches_serde_json() {
    // 127 containers plus a scalar = 128-deep root-to-leaf: the
    // deepest shape `serde_json` accepts, and so do we.
    assert!(parse_strict_json(&nested_arrays(MAX_JSON_NESTING_DEPTH - 1)).is_ok());
    // The 128th nested container trips our typed budget before
    // either parser can recurse past it, hostile depth included.
    for depth in [MAX_JSON_NESTING_DEPTH, MAX_JSON_NESTING_DEPTH + 1, 10_000] {
        let err = parse_strict_json(&nested_arrays(depth)).expect_err("too deep");
        assert!(
            matches!(
                err,
                ContractError::DocumentTooDeep { depth: got, limit: MAX_JSON_NESTING_DEPTH }
                if got == MAX_JSON_NESTING_DEPTH
            ),
            "wrong error for depth {depth}: {err:?}"
        );
    }
}

#[test]
fn nesting_budget_counts_objects_and_mixed_shapes() {
    let mut objects = String::new();
    for level in 0..MAX_JSON_NESTING_DEPTH - 1 {
        write!(objects, r#"{{"k{level}":"#).expect("buffer");
    }
    objects.push('1');
    objects.push_str(&"}".repeat(MAX_JSON_NESTING_DEPTH - 1));
    assert!(parse_strict_json(&objects).is_ok());
    let mixed = format!(r#"{{"a":{}}}"#, nested_arrays(MAX_JSON_NESTING_DEPTH - 1));
    let err = parse_strict_json(&mixed).expect_err("mixed too deep");
    assert!(
        matches!(err, ContractError::DocumentTooDeep { .. }),
        "wrong error: {err:?}"
    );
}
