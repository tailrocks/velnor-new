use super::valid_merge_commit_sha;

#[test]
fn optional_pull_request_merge_sha_is_absent_or_canonical() {
    assert!(valid_merge_commit_sha(None));
    assert!(valid_merge_commit_sha(Some(&"a".repeat(40))));
    assert!(!valid_merge_commit_sha(Some(&"A".repeat(40))));
    assert!(!valid_merge_commit_sha(Some(&"a".repeat(39))));
    assert!(!valid_merge_commit_sha(Some(&format!(
        "{}g",
        "a".repeat(39)
    ))));
}
