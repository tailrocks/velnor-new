use super::*;

/// Fresh scratch root, cleared when a previous run left it behind.
fn scratch(test: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("velnor-guards-{test}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn anchor_splits_existing_prefix_from_clean_suffix() {
    let root = scratch("anchor").expect("scratch");
    let (anchor, suffix) = preview_anchor(&root.join("new-nested/deep"), "label").expect("anchor");
    assert_eq!(
        anchor,
        root.canonicalize().expect("canonical"),
        "anchor is the canonical parent"
    );
    assert_eq!(suffix, PathBuf::from("new-nested/deep"));
    let (anchor, suffix) = preview_anchor(&root, "label").expect("self anchor");
    assert_eq!(anchor, root.canonicalize().expect("canonical"));
    assert!(suffix.as_os_str().is_empty(), "no remainder");
}

#[cfg(unix)]
#[test]
fn anchor_rejects_symlink_components() {
    let root = scratch("anchor-link").expect("scratch");
    let link = root.join("link");
    std::os::unix::fs::symlink(root.join("target"), &link).expect("symlink");
    let err = preview_anchor(&link.join("sub"), "label").expect_err("link refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
}

#[test]
fn refuse_matrix_covers_inside_ancestor_and_sibling() {
    let root = PathBuf::from("/repo");
    for (candidate, reason) in [
        ("/repo", "inside_repository"),
        ("/repo/sub", "inside_repository"),
        ("/", "ancestor_of_repository"),
    ] {
        let err = refuse_inside_or_ancestor(&root, &PathBuf::from(candidate), "label")
            .expect_err("refused");
        assert!(err.to_string().contains(reason), "{err}");
    }
    for candidate in ["/other", "/repo2"] {
        assert!(
            refuse_inside_or_ancestor(&root, &PathBuf::from(candidate), "label").is_ok(),
            "{candidate} is outside"
        );
    }
}

#[test]
fn reserve_rejects_raced_github() {
    let root = scratch("reserve").expect("scratch");
    std::fs::create_dir(root.join(".github")).expect("pre-created");
    let err = reserve_preview_github(&root, "label").expect_err("raced");
    assert!(err.to_string().contains("concurrent_preview"), "{err}");
    let fresh = scratch("reserve-clean").expect("scratch");
    assert!(reserve_preview_github(&fresh, "label").is_ok());
    assert!(fresh.join(".github").is_dir(), "reserved");
}

#[test]
fn ownership_is_exclusive_until_released() {
    let root = scratch("ownership").expect("scratch");
    let first = GenerateOwnership::acquire(&root).expect("first");
    let err = GenerateOwnership::acquire(&root).expect_err("second refused");
    assert!(err.to_string().contains("concurrent_generate"), "{err}");
    drop(first);
    assert!(
        !root.join(".github.velnor-generate.lock").exists(),
        "released on drop"
    );
    let _second = GenerateOwnership::acquire(&root).expect("re-acquire");
}
