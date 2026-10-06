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
