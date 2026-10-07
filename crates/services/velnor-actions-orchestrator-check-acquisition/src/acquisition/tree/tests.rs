use super::*;
use std::time::Duration;

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(60)).expect("deadline")
}

fn source() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp");
    fs::create_dir(root.path().join("bin")).expect("bin");
    fs::write(root.path().join("bin/tool"), b"tool").expect("tool");
    fs::write(root.path().join("README"), b"readme").expect("readme");
    root
}
#[test]
fn positive_hash_and_freeze() {
    let held = source();
    let root = held.path().canonicalize().expect("canonical root");
    let expected = tree_sha256(&root, deadline()).expect("hash");
    let entries = collect(&root, deadline()).expect("entries");
    let manifest = entries
        .iter()
        .map(|entry| match entry {
            Entry::File(file) => json!({"path": &file.path, "kind": "file", "sha256": &file.sha256, "executable": file.executable}),
            Entry::Symlink(path, target) => json!({"path": path, "kind": "symlink", "target": target}),
            Entry::Directory(path) => json!({"path": path, "kind": "directory"}),
        })
        .collect::<Vec<_>>();
    let previous_bytes = canonical_json_bytes(&manifest).expect("manifest");
    let previous = digest_hex(&Sha256::digest(previous_bytes));
    assert_eq!(
        expected, previous,
        "streaming preserves the locked tree recipe"
    );
    freeze_tree(&root, deadline()).expect("freeze");
    assert_eq!(
        tree_sha256(&root, deadline()).expect("frozen hash"),
        expected
    );
    assert_eq!(fs::read(root.join("bin/tool")).expect("tool"), b"tool");
}
#[test]
fn tamper_changes_identity() {
    let root = source();
    let expected = tree_sha256(root.path(), deadline()).expect("hash");
    fs::write(root.path().join("README"), b"tampered").expect("tamper");
    assert_ne!(
        tree_sha256(root.path(), deadline()).expect("tampered hash"),
        expected
    );
}

#[cfg(unix)]
#[test]
fn links_require_in_root_targets_and_survive_copy() {
    let root = source();
    std::os::unix::fs::symlink("tool", root.path().join("bin/link")).expect("link");
    freeze_tree(root.path(), deadline()).expect("freeze");
    assert_eq!(
        fs::read_link(root.path().join("bin/link")).expect("link"),
        Path::new("tool")
    );
    let bad = source();
    std::os::unix::fs::symlink("../../outside", bad.path().join("escape")).expect("escape");
    assert!(tree_sha256(bad.path(), deadline()).is_err());
}
