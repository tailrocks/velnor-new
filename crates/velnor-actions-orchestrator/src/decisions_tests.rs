//! Decision unit tests: the test-only restore-ownership gate.
//!
//! Declared via `#[path]` from `decisions.rs` under `cfg(test)` so the
//! decisions module keeps its size gate.

use super::*;
use velnor_actions_contract::digest_b3;

/// Fully observed restore: real path, bytes, and matching digests.
fn observed_restore() -> velnor_actions_mise::restore_evidence::RestoreObservation {
    use velnor_actions_mise::restore_evidence::RestoreObservation;
    let bytes = b"entry bytes".to_vec();
    RestoreObservation {
        entry_path: "task-artifacts/v2/clippy/entry".to_owned(),
        entry_bytes: bytes.clone(),
        expected_digest: digest_b3(&bytes),
        expected_compat: digest_b3(b"compat"),
        observed_compat: digest_b3(b"compat"),
        expected_owner: "trusted".to_owned(),
        observed_owner: "trusted".to_owned(),
        expected_inputs: digest_b3(b"inputs"),
        observed_inputs: digest_b3(b"inputs"),
    }
}

/// Trust maps to exactly the two live owner scopes.
#[test]
fn trust_maps_to_live_scopes_only() {
    assert_eq!(owner_scope_for_trust(Trust::Trusted), "trusted");
    assert_eq!(owner_scope_for_trust(Trust::Pr), "pr");
}

/// The gate checks Trust-derived ownership first, then the ordered
/// evidence checks; every failure carries its precise reason.
#[test]
fn restore_checks_ownership_explicitly() {
    let obs = observed_restore();
    assert_eq!(
        classify_restore_with_ownership(Trust::Pr, &obs),
        Err("ownership_mismatch")
    );
    assert!(classify_restore_with_ownership(Trust::Trusted, &obs).is_ok());
    let mut obs = observed_restore();
    obs.entry_path.clear();
    assert_eq!(
        classify_restore_with_ownership(Trust::Trusted, &obs),
        Err("no_entry")
    );
    let mut obs = observed_restore();
    obs.entry_bytes = b"forged".to_vec();
    assert_eq!(
        classify_restore_with_ownership(Trust::Trusted, &obs),
        Err("cache_corrupt")
    );
    let mut obs = observed_restore();
    obs.observed_owner = "pr".to_owned();
    assert_eq!(
        classify_restore_with_ownership(Trust::Pr, &obs),
        Err("trust_scope_mismatch")
    );
}

/// Tofu lockfiles never broaden: calling-root selection attributes per root.
#[test]
fn tofu_paths_never_broaden_selection() {
    for path in [
        "stacks/a/.terraform.lock.hcl",
        "stacks/a/main.tf",
        "terraform.tfvars",
        "a.auto.tfvars",
    ] {
        assert_eq!(broadening_for_path(path), None, "{path}");
    }
}

/// Generation-time image evidence is unobserved, never label-split.
#[test]
fn runner_image_evidence_is_unobserved() {
    let evidence = runner_image_evidence();
    assert!(evidence.is_unobserved());
    assert!(evidence.validate().is_ok());
    assert!(RunnerImageEvidence::observed("ubuntu", "26.04").is_ok());
    assert!(RunnerImageEvidence::observed("unknown", "20260928.1.0").is_err());
    assert!(RunnerImageEvidence::observed("ubuntu26", "unknown").is_err());
}
