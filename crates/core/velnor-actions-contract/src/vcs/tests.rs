use std::collections::BTreeMap;

use super::VcsInputs;

#[test]
fn commit_width_rejects_empty_and_uppercase() {
    let vcs = |commit: Option<&str>| VcsInputs {
        commit: commit.map(str::to_owned),
        reference: None,
        submodules: BTreeMap::new(),
    };
    assert!(vcs(Some(&"a".repeat(40))).validate().is_ok());
    assert!(vcs(Some("")).validate().is_err());
    assert!(vcs(Some(&"A".repeat(40))).validate().is_err());
    assert!(vcs(Some("abc")).validate().is_err());
}

fn vcs_with_reference(reference: &str) -> VcsInputs {
    VcsInputs {
        commit: None,
        reference: Some(reference.to_owned()),
        submodules: BTreeMap::new(),
    }
}

#[test]
fn reference_accepts_git_valid_full_refs() {
    for reference in [
        "refs/heads/main",
        "refs/heads/release/1.2",
        "refs/heads/feature/x_y-z",
        "refs/heads/main.LOCK",
        "refs/heads/a/b.Lock",
        "refs/remotes/origin/a/-nested",
        "refs/heads/a@b",
        "refs/heads/@",
        "refs/heads/a]b",
        "refs/heads/a./b",
        "refs/heads/máin",
        "refs/heads/-main",
        "refs/heads/HEAD",
        "refs/tags/v1.0+meta",
    ] {
        assert!(
            vcs_with_reference(reference).validate().is_ok(),
            "{reference:?}"
        );
    }
}

#[test]
fn reference_rejects_git_invalid_and_special_names() {
    for reference in [
        "",
        "main",
        "HEAD",
        "@",
        "/refs/heads/main",
        "refs/heads/main/",
        "refs//heads/main",
        "refs/heads/.hidden",
        "refs/heads/main.",
        "refs/heads/main.lock",
        "refs/heads/a/b.lock",
        "refs/heads/a.lock/child",
        "refs/heads/a..b",
        "refs/heads/a/../b",
        "refs/heads/a/./b",
        "refs/heads/a@{b",
        "@{-1}",
        "refs/heads/a b",
        "refs/heads/a\nb",
        "refs/heads/a\tb",
        "refs/heads/a\0b",
        "refs/heads/a\u{7f}b",
        "refs/heads/a~b",
        "refs/heads/a^b",
        "refs/heads/a:b",
        "refs/heads/a?b",
        "refs/heads/a*b",
        "refs/heads/a[b",
        "refs/heads/a\\b",
    ] {
        assert!(
            vcs_with_reference(reference).validate().is_err(),
            "{reference:?}"
        );
    }
}
