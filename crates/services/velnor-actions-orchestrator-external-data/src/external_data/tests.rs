use super::*;
use velnor_actions_contract::digest_b3;

/// Freshness proof with `age_secs` over a fixed identity.
fn proof(age_secs: u64) -> ExternalDataFreshness {
    ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: digest_b3(b"snapshot"),
        age_secs,
    }
}

#[test]
fn skip_needs_declared_identity_and_fresh_baseline() {
    let fresh = proof(60);
    assert!(may_skip_external_data(true, Some(&fresh), 86_400));
    assert!(!may_skip_external_data(false, Some(&fresh), 86_400));
    assert!(!may_skip_external_data(true, None, 86_400));
    assert!(!may_skip_external_data(true, Some(&proof(86_401)), 86_400));
    let mut bad = proof(60);
    bad.identity = "not-a-digest".to_owned();
    assert!(!may_skip_external_data(true, Some(&bad), 86_400));
}

#[test]
fn advisory_kind_classifies_stack_and_internal_ids() {
    assert_eq!(
        external_data_kind("stack/rust/root/advisory/default"),
        Some("advisory")
    );
    assert_eq!(
        external_data_kind("internal/advisory/default"),
        Some("advisory")
    );
    assert_eq!(external_data_kind("stack/rust/root/clippy/default"), None);
    assert_eq!(external_data_kind("bogus"), None);
}

#[test]
fn freshness_validation_rejects_bad_source_and_identity() {
    assert!(proof(0).validate().is_ok());
    let mut bad = proof(0);
    bad.source = String::new();
    assert!(bad.validate().is_err());
    bad.source = "has space".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = proof(0);
    bad.identity = "b3-short".to_owned();
    assert!(bad.validate().is_err());
}
