use super::snapshot::{SnapshotEntry, sha256, snapshot_tree, validate_formula_only_delta};
use std::collections::BTreeMap;

fn entry(bytes: &[u8], mode: u32) -> SnapshotEntry {
    SnapshotEntry {
        mode,
        sha256: sha256(bytes),
        size: bytes.len() as u64,
        directory: false,
    }
}

#[test]
fn formula_bytes_are_the_only_accepted_tree_change() {
    let before = BTreeMap::from([
        ("Formula/jackin.rb".to_owned(), entry(b"old formula", 0o644)),
        ("mise.toml".to_owned(), entry(b"task source", 0o644)),
        (
            ".git/HEAD".to_owned(),
            entry(b"ref: refs/heads/main\n", 0o644),
        ),
    ]);
    let mut after = before.clone();
    after.insert("Formula/jackin.rb".to_owned(), entry(b"new formula", 0o644));
    assert!(
        validate_formula_only_delta(&before, &after, "Formula/jackin.rb", b"new formula").is_ok()
    );
}

#[test]
fn extra_removed_nonformula_and_git_base_changes_are_rejected() {
    let before = BTreeMap::from([
        ("Formula/jackin.rb".to_owned(), entry(b"old", 0o644)),
        ("mise.toml".to_owned(), entry(b"task", 0o644)),
        (".git/refs/heads/main".to_owned(), entry(b"base sha", 0o644)),
    ]);
    let mut extra = before.clone();
    extra.insert("Formula/other.rb".to_owned(), entry(b"injected", 0o644));
    assert!(validate_formula_only_delta(&before, &extra, "Formula/jackin.rb", b"new").is_err());
    let mut removed = before.clone();
    removed.remove("mise.toml");
    assert!(validate_formula_only_delta(&before, &removed, "Formula/jackin.rb", b"new").is_err());
    let mut changed_base = before.clone();
    changed_base.insert(
        ".git/refs/heads/main".to_owned(),
        entry(b"replacement sha", 0o644),
    );
    assert!(
        validate_formula_only_delta(&before, &changed_base, "Formula/jackin.rb", b"new").is_err()
    );
}

#[test]
fn executable_mode_change_is_rejected_even_for_the_formula() {
    let before = BTreeMap::from([("Formula/jackin.rb".to_owned(), entry(b"old", 0o644))]);
    let after = BTreeMap::from([("Formula/jackin.rb".to_owned(), entry(b"new", 0o755))]);
    assert!(validate_formula_only_delta(&before, &after, "Formula/jackin.rb", b"new").is_err());
}

#[cfg(unix)]
#[test]
fn snapshot_refuses_symlinks_anywhere_in_tap_checkout() {
    let root = tempfile::TempDir::new().expect("temp root");
    std::os::unix::fs::symlink("elsewhere", root.path().join("Formula")).expect("symlink");
    assert!(snapshot_tree(root.path()).is_err());
}
