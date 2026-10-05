//! actionlint shell integration cases.
#[test]
fn tool_id_is_actionlint() {
    assert_eq!(velnor_actions_actionlint::TOOL_ID, "actionlint");
}
