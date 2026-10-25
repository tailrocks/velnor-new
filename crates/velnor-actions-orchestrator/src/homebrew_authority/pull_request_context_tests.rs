use super::{merge_parents_match, valid_sha};

#[test]
fn authenticated_merge_checkout_requires_exact_ordered_base_and_head_parents() {
    let base = "a".repeat(40);
    let head = "b".repeat(40);
    assert!(merge_parents_match(
        &[base.clone(), head.clone()],
        &base,
        &head
    ));
    assert!(!merge_parents_match(
        &[head.clone(), base.clone()],
        &base,
        &head
    ));
    assert!(!merge_parents_match(
        &[base.clone(), head.clone(), "c".repeat(40)],
        &base,
        &head
    ));
    assert!(!merge_parents_match(
        std::slice::from_ref(&base),
        &base,
        &head
    ));
}

#[test]
fn commit_sha_acceptance_is_lowercase_and_exactly_sha1_width() {
    assert!(valid_sha(Some(&"a".repeat(40))));
    assert!(!valid_sha(Some(&"A".repeat(40))));
    assert!(!valid_sha(Some(&"a".repeat(39))));
    assert!(!valid_sha(None));
}
