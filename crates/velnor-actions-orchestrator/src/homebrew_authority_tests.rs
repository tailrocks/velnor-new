use super::admit_pull_request_context;

fn pull() -> crate::package_release_publish::PublicPullRequestSnapshot {
    crate::package_release_publish::PublicPullRequestSnapshot {
        state: "open".to_owned(),
        draft: false,
        merged: false,
        base_repository: "owner/tap".to_owned(),
        base_repository_id: 7,
        base_ref: "main".to_owned(),
        base_sha: "a".repeat(40),
        head_repository: "owner/tap".to_owned(),
        head_repository_id: 7,
        head_ref: "change".to_owned(),
        head_sha: "b".repeat(40),
    }
}

#[test]
fn locator_context_uses_only_an_open_same_repository_main_base_and_exact_head() {
    let accepted = pull();
    assert_eq!(
        admit_pull_request_context(&accepted, "owner/tap", 7, &"b".repeat(40))
            .expect("authenticated PR"),
        ("a".repeat(40), "b".repeat(40))
    );

    let mut rejected = pull();
    rejected.base_ref = "release".to_owned();
    assert!(admit_pull_request_context(&rejected, "owner/tap", 7, &"b".repeat(40)).is_err());
    rejected = pull();
    rejected.head_repository = "fork/tap".to_owned();
    assert!(admit_pull_request_context(&rejected, "owner/tap", 7, &"b".repeat(40)).is_err());
    rejected = pull();
    rejected.draft = true;
    assert!(admit_pull_request_context(&rejected, "owner/tap", 7, &"b".repeat(40)).is_err());
    rejected = pull();
    rejected.head_sha = "c".repeat(40);
    assert!(admit_pull_request_context(&rejected, "owner/tap", 7, &"b".repeat(40)).is_err());
}
