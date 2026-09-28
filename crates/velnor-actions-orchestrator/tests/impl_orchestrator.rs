//! Orchestrator shell integration cases.
#[test]
fn orchestrator_version_marker_is_zero_at_gate_0() {
    assert_eq!(velnor_actions_orchestrator::ORCHESTRATOR_VERSION, 0);
}
