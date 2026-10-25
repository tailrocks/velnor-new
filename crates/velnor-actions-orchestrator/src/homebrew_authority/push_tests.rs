use super::push_context_matches;

#[test]
fn local_cutover_push_uses_authenticated_tip_and_exact_checkout() {
    let tip = "a".repeat(40);
    assert!(push_context_matches(&tip, &tip, &tip));
    assert!(!push_context_matches(&tip, &"b".repeat(40), &tip));
    assert!(!push_context_matches(&tip, &tip, &"b".repeat(40)));
}
