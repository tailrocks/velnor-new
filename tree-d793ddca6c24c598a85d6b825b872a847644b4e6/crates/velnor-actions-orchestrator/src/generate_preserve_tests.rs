//! Ownership and transactional preservation regressions.

use std::fs;
use std::path::Path;

use super::copy_repository_content;
use crate::generate::{swap_directories, write_tree};
use velnor_actions_workflow_renderer::render::{RenderedFile, RenderedTree};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn replacement(root: &Path) -> TestResult {
    replace_with_cleanup_policy(root, false)
}

fn replace_with_cleanup_policy(root: &Path, readonly_backup: bool) -> TestResult {
    let staging = tempfile::tempdir_in(root)?;
    let permissions = super::root_permissions(&root.join(".github"))?;
    // Existing roots stage as siblings: read-only directory moves must stay
    // within one parent on macOS. Restore the final mode before publication.
    let staged = if permissions.is_some() {
        staging.path().to_path_buf()
    } else {
        staging.path().join(".github")
    };
    let directories = copy_repository_content(&root.join(".github"), &staged)?;
    let tree = RenderedTree {
        files: vec![RenderedFile {
            path: ".github/workflows/ci.yml".to_owned(),
            bytes: "new workflow\n".to_owned(),
        }],
        symlinks: Vec::new(),
    };
    super::check_generated_collisions(&staged, &tree)?;
    write_tree(&staged, &tree)?;
    super::restore_directory_permissions(directories)?;
    super::restore_root_permissions(&staged, permissions)?;
    let warnings = swap_directories(root, &root.join(".github"), &staged)?;
    if readonly_backup {
        assert!(
            warnings
                .iter()
                .all(|warning| warning.starts_with("backup_cleanup_failed:"))
        );
    } else {
        assert_eq!(warnings, [] as [String; 0]);
    }
    Ok(())
}

#[test]
fn replacement_preserves_unowned_tree_and_removes_only_owned_paths() -> TestResult {
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(github.join("ISSUE_TEMPLATE/empty"))?;
    fs::create_dir_all(github.join("workflows/nested"))?;
    for path in [
        "workflows/old.yml",
        "workflows/nested/old.yml",
        "AGENTS.md",
        "CLAUDE.md",
        "actionlint.yaml",
        "release-plz.toml",
        "release-plz-bootstrap.toml",
    ] {
        fs::write(github.join(path), b"obsolete")?;
    }
    for path in [
        "CODEOWNERS",
        "ISSUE_TEMPLATE/bug.yml",
        ".hidden",
        "release-plz-local.toml",
    ] {
        fs::write(github.join(path), b"repository content\0\xff")?;
    }
    for _ in 0..2 {
        replacement(root.path())?;
        for path in [
            "CODEOWNERS",
            "ISSUE_TEMPLATE/bug.yml",
            ".hidden",
            "release-plz-local.toml",
        ] {
            assert_eq!(fs::read(github.join(path))?, b"repository content\0\xff");
        }
        assert!(github.join("ISSUE_TEMPLATE/empty").is_dir());
        assert_eq!(
            fs::read(github.join("workflows/ci.yml"))?,
            b"new workflow\n"
        );
        for path in [
            "workflows/old.yml",
            "AGENTS.md",
            "CLAUDE.md",
            "actionlint.yaml",
            "release-plz.toml",
            "release-plz-bootstrap.toml",
        ] {
            assert!(
                !github.join(path).exists(),
                "owned stale path survives: {path}"
            );
        }
        assert!(!github.join("workflows/nested").exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn preservation_keeps_modes_and_literal_symlink_targets() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(github.join("scripts"))?;
    fs::write(github.join("scripts/check"), b"#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(
        github.join("scripts/check"),
        fs::Permissions::from_mode(0o751),
    )?;
    fs::set_permissions(github.join("scripts"), fs::Permissions::from_mode(0o750))?;
    symlink("../missing", github.join("dangling"))?;
    symlink(root.path(), github.join("directory-link"))?;
    replacement(root.path())?;
    assert_eq!(
        fs::metadata(github.join("scripts/check"))?
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
    assert_eq!(
        fs::metadata(github.join("scripts"))?.permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::read_link(github.join("dangling"))?,
        Path::new("../missing")
    );
    assert_eq!(fs::read_link(github.join("directory-link"))?, root.path());
    assert!(fs::symlink_metadata(github.join("directory-link"))?.is_symlink());
    Ok(())
}

#[cfg(unix)]
#[test]
fn unsupported_entry_refuses_before_replacement() -> TestResult {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};

    // Unix socket paths are bounded by the kernel, independently of TMPDIR.
    let short_parent = Path::new("/tmp").canonicalize()?;
    let root = tempfile::tempdir_in(short_parent)?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
    assert_eq!(
        fs::metadata(root.path())?.permissions().mode() & 0o777,
        0o700
    );
    let github = root.path().join(".github");
    fs::create_dir_all(&github)?;
    fs::write(github.join("CODEOWNERS"), b"kept")?;
    let socket = github.join("socket");
    assert!(socket.as_os_str().as_bytes().len() < 100);
    let _socket = std::os::unix::net::UnixListener::bind(&socket)?;
    assert!(fs::symlink_metadata(&socket)?.file_type().is_socket());
    let error = replacement(root.path()).expect_err("socket cannot be preserved safely");
    assert!(
        error.to_string().contains("unsupported_repository_entry"),
        "{error}"
    );
    assert_eq!(fs::read(github.join("CODEOWNERS"))?, b"kept");
    assert!(!github.join("workflows").exists());
    Ok(())
}

#[test]
fn repository_file_collision_refuses_before_replacement() -> TestResult {
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(github.join("custom"))?;
    fs::write(github.join("custom/support.py"), b"repository bytes")?;
    let staging = tempfile::tempdir_in(root.path())?;
    let staged = staging.path().join(".github");
    let _directories = copy_repository_content(&github, &staged)?;
    let tree = RenderedTree {
        files: vec![RenderedFile {
            path: ".github/custom/support.py".to_owned(),
            bytes: "generated bytes".to_owned(),
        }],
        symlinks: Vec::new(),
    };
    let error = super::check_generated_collisions(&staged, &tree)
        .expect_err("repository collision refuses");
    assert!(matches!(
        error,
        crate::OrchestratorError::OverwriteRefused { .. }
    ));
    assert_eq!(
        fs::read(github.join("custom/support.py"))?,
        b"repository bytes"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn repository_symlink_collision_refuses_before_replacement() -> TestResult {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(github.join("custom"))?;
    fs::write(github.join("CODEOWNERS"), b"repository owners")?;
    symlink(outside.path(), github.join("custom/link"))?;
    let staging = tempfile::tempdir_in(root.path())?;
    let staged = staging.path().join(".github");
    let _directories = copy_repository_content(&github, &staged)?;
    let tree = RenderedTree {
        files: vec![RenderedFile {
            path: ".github/custom/link/escaped/new/output.py".to_owned(),
            bytes: "must not write outside".to_owned(),
        }],
        symlinks: Vec::new(),
    };
    let error =
        super::check_generated_collisions(&staged, &tree).expect_err("symlink ancestor refuses");
    assert!(matches!(error, crate::OrchestratorError::UnsafePath { .. }));
    assert!(!outside.path().join("escaped").exists());
    assert_eq!(fs::read(github.join("CODEOWNERS"))?, b"repository owners");
    assert!(fs::symlink_metadata(github.join("custom/link"))?.is_symlink());
    Ok(())
}

#[cfg(unix)]
#[test]
fn repository_github_root_symlink_refuses() -> TestResult {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    symlink(outside.path(), root.path().join(".github"))?;
    let error = copy_repository_content(&root.path().join(".github"), &root.path().join("staged"))
        .expect_err("repository root symlink refuses");
    assert!(matches!(error, crate::OrchestratorError::UnsafePath { .. }));
    assert!(!root.path().join("staged").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn replacement_preserves_readonly_root_mode() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(&github)?;
    fs::write(github.join("CODEOWNERS"), b"owners")?;
    fs::set_permissions(&github, fs::Permissions::from_mode(0o555))?;
    let outcome = replace_with_cleanup_policy(root.path(), true);
    let mode = fs::metadata(&github)?.permissions().mode() & 0o777;
    fs::set_permissions(&github, fs::Permissions::from_mode(0o755))?;
    outcome?;
    assert_eq!(mode, 0o555);
    assert_eq!(fs::read(github.join("CODEOWNERS"))?, b"owners");
    Ok(())
}

#[cfg(unix)]
#[test]
fn readonly_repository_directory_mode_is_restored_after_new_output() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(github.join("custom"))?;
    fs::write(github.join("custom/kept"), b"kept")?;
    fs::set_permissions(github.join("custom"), fs::Permissions::from_mode(0o555))?;
    let staging = tempfile::tempdir_in(root.path())?;
    let staged = staging.path().join(".github");
    let directories = copy_repository_content(&github, &staged)?;
    let tree = RenderedTree {
        files: vec![RenderedFile {
            path: ".github/custom/new.yml".to_owned(),
            bytes: "new output".to_owned(),
        }],
        symlinks: Vec::new(),
    };
    super::check_generated_collisions(&staged, &tree)?;
    write_tree(&staged, &tree)?;
    super::restore_directory_permissions(directories)?;
    let mode = fs::metadata(staged.join("custom"))?.permissions().mode() & 0o777;
    fs::set_permissions(staged.join("custom"), fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(github.join("custom"), fs::Permissions::from_mode(0o755))?;
    assert_eq!(mode, 0o555);
    assert_eq!(fs::read(staged.join("custom/kept"))?, b"kept");
    assert_eq!(fs::read(staged.join("custom/new.yml"))?, b"new output");
    Ok(())
}
