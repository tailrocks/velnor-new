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

#[test]
fn rollback_failure_preserved() {
    let root = scratch("rollback_fail").expect("scratch");
    let target = root.join(".github");
    std::fs::create_dir_all(&target).expect("target");
    std::fs::write(target.join("old.yml"), "old: true\n").expect("old");
    let outcome = restore_backup(&root.join("missing-backup"), &target);
    assert!(outcome.starts_with("rollback_failed:"), "{outcome}");
    assert_eq!(
        std::fs::read(target.join("old.yml")).expect("kept"),
        b"old: true\n"
    );
}

mod preserved_template;
