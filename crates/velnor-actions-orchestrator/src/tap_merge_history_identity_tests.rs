use super::{commit_matches, validate_observed_base};
use serde_json::json;

#[test]
fn accepts_github_git_commit_response_shape_with_top_level_message() {
    let head = "a".repeat(40);
    let base = "b".repeat(40);
    let commit = json!({
        "sha": head,
        "message": "subject\n\nbody",
        "parents": [{"sha": base}],
        "commit": {"message": "different nested git-database shape"}
    });
    assert!(commit_matches(&commit, &head, &base, "subject\n\nbody"));

    let incorrect_shape = json!({
        "sha": head,
        "commit": {"message": "subject\n\nbody"},
        "parents": [{"sha": base}]
    });
    assert!(!commit_matches(
        &incorrect_shape,
        &head,
        &base,
        "subject\n\nbody"
    ));
}

#[test]
fn changed_base_on_closed_unmerged_history_does_not_rewrite_original_parent() {
    let original = "a".repeat(40);
    let advanced = "b".repeat(40);
    let closed = json!({"state": "closed", "base": {"sha": advanced}});
    assert!(validate_observed_base(&closed, &original, false).is_ok());

    let merged = json!({"state": "closed", "base": {"sha": advanced}});
    assert!(validate_observed_base(&merged, &original, true).is_err());

    let open = json!({"state": "open", "base": {"sha": advanced}});
    assert!(validate_observed_base(&open, &original, false).is_err());
}

#[test]
fn requires_observed_base_to_be_a_full_sha() {
    let invalid = json!({"state": "closed", "base": {"sha": "main"}});
    assert!(validate_observed_base(&invalid, &"a".repeat(40), false).is_err());
}

#[test]
fn receipt_ownership_ignores_public_markers_outside_its_branch_namespace() {
    use super::{belongs_to_receipt, branch_prefix};

    let digest = format!("b3-{}", "a".repeat(64));
    let prefix = branch_prefix(&digest).expect("valid receipt digest");
    let marker = format!("Receipt digest: `{digest}`");

    assert!(
        belongs_to_receipt("feature/unrelated", &marker, &prefix, &marker)
            .is_ok_and(|owned| !owned)
    );
    assert!(
        belongs_to_receipt(
            &format!("{prefix}{}", "b".repeat(40)),
            "missing marker",
            &prefix,
            &marker,
        )
        .is_err()
    );
    assert!(
        belongs_to_receipt(
            &format!("{prefix}{}", "b".repeat(40)),
            &marker,
            &prefix,
            &marker,
        )
        .is_ok_and(|owned| owned)
    );
}
