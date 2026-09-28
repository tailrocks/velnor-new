//! Contract shell integration cases.
#[test]
fn contract_version_marker_is_zero_at_gate_0() {
    assert_eq!(velnor_actions_contract::CONTRACT_VERSION, 0);
}
