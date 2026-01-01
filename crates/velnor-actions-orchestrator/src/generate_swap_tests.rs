use super::*;

fn scratch(test: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("velnor-gen-{test}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn commit_failure_preserves_old_tree_without_partial_write() {
    let root = scratch("commit_fail").expect("scratch");
    let target = root.join(".github");
    std::fs::create_dir_all(target.join("workflows")).expect("target");
    std::fs::write(target.join("workflows/old.yml"), "old: true\n").expect("old");
    let err = swap_directories(&root, &target, &root.join("missing-staged")).expect_err("fails");
    assert!(
        !target.join("actionlint.yaml").exists(),
        "no partial write: {err}"
    );
    assert_eq!(
        std::fs::read(target.join("workflows/old.yml")).expect("kept"),
        b"old: true\n"
    );
}

#[test]
fn fresh_target_commit_failure_reports_without_rollback() {
    let root = scratch("fresh_commit_fail").expect("scratch");
    let target = root.join(".github");
    let err = swap_directories(&root, &target, &root.join("missing-staged")).expect_err("fails");
    assert!(!err.to_string().contains("rolled_back"), "{err}");
    assert!(!target.exists(), "nothing committed");
}

#[cfg(unix)]
fn marker_hash(path: &Path) -> std::io::Result<String> {
    use std::os::unix::fs::PermissionsExt;

    let mut bytes = std::fs::read(path)?;
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    bytes.extend_from_slice(&mode.to_le_bytes());
    Ok(velnor_actions_contract::digest_b3(&bytes))
}

#[cfg(unix)]
#[test]
fn existing_target_exchange_refusal_preserves_old_unmanaged_tree() {
    use std::os::unix::fs::PermissionsExt;

    let root = scratch("exchange_refused").expect("scratch");
    let target = root.join(".github");
    let staged = root.join("staged");
    let marker = target.join("local/marker.bin");
    std::fs::create_dir_all(marker.parent().expect("marker parent")).expect("target");
    std::fs::write(&marker, b"unmanaged\0marker\xff").expect("marker");
    let mut mode = std::fs::metadata(&marker)
        .expect("marker metadata")
        .permissions();
    mode.set_mode(0o640);
    std::fs::set_permissions(&marker, mode).expect("marker mode");
    let before = marker_hash(&marker).expect("marker state");
    std::fs::create_dir_all(&staged).expect("staged");
    std::fs::write(staged.join("new.yml"), "new: true\n").expect("new");
    let err = swap_existing_directory(&target, &staged, |_, _| {
        assert!(target.is_dir(), "target stays visible at exchange refusal");
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "forced_exchange_refusal",
        ))
    })
    .expect_err("exchange refusal must fail closed");
    assert!(err.to_string().contains("forced_exchange_refusal"), "{err}");
    assert!(target.is_dir(), "existing root remains visible");
    assert!(staged.is_dir(), "staged tree remains for owner cleanup");
    assert_eq!(marker_hash(&marker).expect("marker state"), before);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn existing_target_successfully_exchanges_tree() {
    let root = scratch("exchange_success").expect("scratch");
    let target = root.join(".github");
    let staged = root.join("staged");
    std::fs::create_dir_all(target.join("workflows")).expect("target");
    std::fs::write(target.join("workflows/old.yml"), "old: true\n").expect("old");
    std::fs::create_dir_all(&staged).expect("staged");
    std::fs::write(staged.join("new.yml"), "new: true\n").expect("new");
    let warnings = swap_directories(&root, &target, &staged).expect("exchange");
    assert!(
        warnings.is_empty(),
        "clean exchange warns nothing: {warnings:?}"
    );
    assert!(!staged.exists(), "exchanged old tree removed");
    assert!(
        !target.join("workflows/old.yml").exists(),
        "old tree replaced"
    );
    assert_eq!(
        std::fs::read(target.join("new.yml")).expect("new tree"),
        b"new: true\n"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn held_old_child_lookup_can_fail_after_cleanup_with_live_new_root() {
    use rustix::fs::{Mode, OFlags, open, openat};

    let root = scratch("held_old_child").expect("scratch");
    let target = root.join(".github");
    let staged = root.join("staged");
    std::fs::create_dir_all(target.join("workflows")).expect("target");
    std::fs::write(target.join("workflows/old.yml"), "old: true\n").expect("old");
    std::fs::create_dir_all(&staged).expect("staged");
    std::fs::write(staged.join("new.yml"), "new: true\n").expect("new");
    let held_old = open(&target, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
        .expect("hold old directory");
    swap_directories(&root, &target, &staged).expect("exchange");
    assert!(target.is_dir(), "new root remains present");
    let stale = openat(
        &held_old,
        "workflows/old.yml",
        OFlags::RDONLY,
        Mode::empty(),
    );
    assert!(
        stale.is_err(),
        "cleanup removes child from held old directory"
    );
}
