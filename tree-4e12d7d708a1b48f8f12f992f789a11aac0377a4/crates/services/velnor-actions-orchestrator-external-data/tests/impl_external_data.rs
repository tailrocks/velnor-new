//! Freshness gate: sources, identities, ages, and skip decisions.
use velnor_actions_orchestrator_external_data::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, EXTERNAL_DATA_CHECK_KIND, ExternalDataFreshness,
    external_data_kind, may_skip_external_data,
};

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

fn proof(age_secs: u64) -> ExternalDataFreshness {
    ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: digest(7),
        age_secs,
    }
}

#[test]
fn check_kind_pins_advisory_segment() {
    assert_eq!(EXTERNAL_DATA_CHECK_KIND, "advisory");
}

#[test]
fn default_max_age_is_one_day() {
    assert_eq!(DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, 86_400);
}

#[test]
fn valid_proof_validates() {
    proof(10).validate().expect("valid proof");
}

#[test]
fn blank_sources_refuse() {
    for source in ["", "   ", "has space", "has/slash", "has\ttab"] {
        let mut bad = proof(10);
        bad.source = source.to_owned();
        assert!(bad.validate().is_err(), "{source:?}");
    }
}

#[test]
fn malformed_identity_refuses() {
    let mut bad = proof(10);
    bad.identity = "b3-short".to_owned();
    assert!(bad.validate().is_err());
}

#[test]
fn advisory_tasks_resolve_kind() {
    assert_eq!(
        external_data_kind("stack/rust/demo/advisory/default"),
        Some("advisory")
    );
}

#[test]
fn other_tasks_resolve_no_kind() {
    for task_id in [
        "stack/rust/demo/clippy/default",
        "stack/rust/demo/test/default",
        "",
    ] {
        assert_eq!(external_data_kind(task_id), None, "{task_id:?}");
    }
}

#[test]
fn undeclared_obligation_never_skips() {
    assert!(!may_skip_external_data(false, Some(&proof(10)), 86_400));
    assert!(!may_skip_external_data(false, None, 86_400));
}

#[test]
fn missing_proof_never_skips() {
    assert!(!may_skip_external_data(true, None, 86_400));
}

#[test]
fn fresh_valid_proof_skips() {
    assert!(may_skip_external_data(true, Some(&proof(10)), 86_400));
    assert!(may_skip_external_data(true, Some(&proof(86_400)), 86_400));
}

#[test]
fn stale_proof_runs_again() {
    assert!(!may_skip_external_data(true, Some(&proof(86_401)), 86_400));
}

#[test]
fn invalid_proof_runs_again() {
    let mut bad = proof(10);
    bad.source = String::new();
    assert!(!may_skip_external_data(true, Some(&bad), 86_400));
    let mut bad = proof(10);
    bad.identity = "b3-short".to_owned();
    assert!(!may_skip_external_data(true, Some(&bad), 86_400));
}
