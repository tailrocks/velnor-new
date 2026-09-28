//! Renderer shell integration cases.
#[test]
fn renderer_version_marker_is_zero_at_gate_0() {
    assert_eq!(velnor_actions_workflow_renderer::RENDERER_VERSION, 0);
}
