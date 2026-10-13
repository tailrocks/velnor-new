//! Output-guard path cases.
use std::path::Path;
use velnor_actions_workflow_renderer::{
    ACTIONLINT_PATH, RenderError, WORKFLOW_PATH, check_no_symlink, join_within_root,
    validate_tree_path,
};

#[test]
fn guard_accepts_generated_tree_paths() -> Result<(), RenderError> {
    assert_eq!(
        validate_tree_path(ACTIONLINT_PATH)?.as_str(),
        ACTIONLINT_PATH
    );
    assert_eq!(validate_tree_path(WORKFLOW_PATH)?.as_str(), WORKFLOW_PATH);
    assert_eq!(
        validate_tree_path(".github/workflows/extra.yml")?.as_str(),
        ".github/workflows/extra.yml"
    );
    Ok(())
}

#[test]
fn guard_rejects_unsafe_paths() {
    for bad in [
        "",
        "/absolute/path.yml",
        ".github/../escape.yml",
        "..",
        ".github//double.yml",
        ".github/./dot.yml",
        ".github\\windows.yml",
        ".github/bad\0byte.yml",
        ".github/bad\nline.yml",
        ".github/trailing/.",
    ] {
        assert!(validate_tree_path(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn guard_join_stays_within_root() -> Result<(), RenderError> {
    let rel = validate_tree_path(WORKFLOW_PATH)?;
    let joined = join_within_root(Path::new("/repo"), &rel);
    assert_eq!(joined, Path::new("/repo/.github/workflows/ci.yml"));
    assert!(joined.starts_with("/repo"));
    Ok(())
}

#[test]
fn guard_symlink_probe_fails_closed() -> Result<(), RenderError> {
    let rel = validate_tree_path(WORKFLOW_PATH)?;
    let root = Path::new("/repo");
    assert!(check_no_symlink(root, &rel, |_| false).is_ok());
    assert!(check_no_symlink(root, &rel, |_| true).is_err());
    let nested_only = |path: &Path| path.ends_with(".github/workflows");
    let err = check_no_symlink(root, &rel, nested_only);
    assert!(err.is_err());
    if let Err(err) = err {
        assert!(format!("{err}").contains("symlink_prefix"));
    }
    Ok(())
}
