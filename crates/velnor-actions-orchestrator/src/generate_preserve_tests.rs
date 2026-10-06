//! Ownership and transactional preservation regressions.

use std::fs;
use std::path::Path;

use super::copy_repository_content;
use crate::generate::{swap_directories, write_tree};
use velnor_actions_workflow_renderer::render::{RenderedFile, RenderedTree};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[path = "generate_profile_ownership_tests.rs"]
mod profiles;

#[path = "generate_retired_ownership_tests.rs"]
mod retired;

fn replacement(root: &Path) -> TestResult {
    let staging = tempfile::tempdir_in(root)?;
    let staged = staging.path().join(".github");
    copy_repository_content(&root.join(".github"), &staged)?;
    let tree = RenderedTree {
        files: vec![RenderedFile {
            path: ".github/workflows/ci.yml".to_owned(),
            bytes: "new workflow\n".to_owned(),
        }],
        symlinks: Vec::new(),
    };
    write_tree(&staged, &tree)?;
    assert!(swap_directories(root, &root.join(".github"), &staged)?.is_empty());
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
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(&github)?;
    fs::write(github.join("CODEOWNERS"), b"kept")?;
    let _socket = std::os::unix::net::UnixListener::bind(github.join("socket"))?;
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
fn nested_delivery_ownership_allows_repeat_and_disabled_cleanup() -> TestResult {
    use crate::apt_delivery::APT_DELIVERY_TREE_PATHS;
    use crate::delivery_emit::oci_delivery::OCI_DELIVERY_TREE_PATHS;
    use velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS;
    let paths: std::collections::BTreeSet<_> = APT_DELIVERY_TREE_PATHS
        .iter()
        .chain(OCI_DELIVERY_TREE_PATHS)
        .chain(RELEASE_TREE_PATHS)
        .copied()
        .collect();
    let root = tempfile::tempdir()?;
    let github = root.path().join(".github");
    fs::create_dir_all(github.join("velnor/custom"))?;
    fs::write(github.join("velnor/custom/helper.py"), b"repository helper")?;
    fs::write(github.join("velnor/local.jsonc"), b"repository config")?;
    for round in 0..3 {
        let staging = tempfile::tempdir_in(root.path())?;
        let staged = staging.path().join(".github");
        copy_repository_content(&github, &staged)?;
        let tree = RenderedTree {
            files: if round < 2 {
                paths
                    .iter()
                    .map(|path| RenderedFile {
                        path: (*path).to_owned(),
                        bytes: format!("delivery generation {round}\n"),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            symlinks: Vec::new(),
        };
        write_tree(&staged, &tree)?;
        assert!(swap_directories(root.path(), &github, &staged)?.is_empty());
        assert_eq!(
            fs::read(github.join("velnor/custom/helper.py"))?,
            b"repository helper"
        );
        assert_eq!(
            fs::read(github.join("velnor/local.jsonc"))?,
            b"repository config"
        );
        for path in &paths {
            let path = root.path().join(path);
            if round < 2 {
                assert_eq!(
                    fs::read_to_string(path)?,
                    format!("delivery generation {round}\n")
                );
            } else {
                assert!(
                    !path.exists(),
                    "disabled delivery retains {}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}
