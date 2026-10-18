use super::{is_full_sha, parse_tree, validate_repository};

#[test]
fn tap_remote_identity_accepts_only_a_single_github_slug_and_full_sha() {
    assert!(validate_repository("jackin-project/homebrew-tap").is_ok());
    assert!(validate_repository("owner/sub.repo").is_ok());
    for invalid in ["", "owner", "owner/repo/extra", "../repo", "owner/../repo"] {
        assert!(validate_repository(invalid).is_err(), "{invalid}");
    }
    assert!(is_full_sha(&"a".repeat(40)));
    assert!(!is_full_sha("a".repeat(39).as_str()));
    assert!(!is_full_sha(&"A".repeat(40)));
}

#[test]
fn captured_tap_tree_accepts_only_regular_unambiguous_entries() {
    let sha = "a".repeat(40);
    let record = format!("100644 blob {sha}\tFormula/jackin.rb\0");
    let tree = parse_tree(record.as_bytes()).expect("regular tree entry");
    assert_eq!(tree[0].path, "Formula/jackin.rb");
    assert_eq!(tree[0].mode, 0o100_644);
    for invalid in [
        format!("120000 blob {sha}\tFormula/link.rb\0"),
        format!("160000 commit {sha}\tsubmodule\0"),
        format!("100644 blob {sha}\t../escape\0"),
        format!("100644 blob {sha}\tFormula/jackin.rb\0{record}"),
    ] {
        assert!(parse_tree(invalid.as_bytes()).is_err(), "{invalid:?}");
    }
}
