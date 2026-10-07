//! Run-key resolution tests: explicit values only.
//!
//! The environment fallback reads ambient `GITHUB_RUN_ID` /
//! `GITHUB_RUN_ATTEMPT` without mutating them, and this repo
//! forbids `set_var` (edition 2024 marks it unsafe), so only the
//! env-free explicit paths pin here; the fallback keeps its
//! existing entrypoint-level coverage.

use super::*;

#[test]
fn explicit_local_key_passes_through() {
    assert_eq!(resolve_run_key(Some("local")).expect("local"), "local");
}

#[test]
fn explicit_ci_key_passes_through() {
    assert_eq!(resolve_run_key(Some("r7-a2")).expect("ci key"), "r7-a2");
}

#[test]
fn explicit_malformed_keys_fail_closed() {
    for key in ["bogus", "r-a", "r1", "r1-a", "r1-a2x", "R1-A2"] {
        assert!(resolve_run_key(Some(key)).is_err(), "{key} fails");
    }
}
