// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate as velnor_actions_orchestrator;

fn test_git() -> crate::crate_release_git::ReleaseGit {
    crate::crate_release_git::ReleaseGit::host_fixture()
}

use std::{fs, path::Path, process::Command};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_orchestrator::verify_crate_release_source;

#[cfg(unix)]
#[test]
fn source_verification_ignores_hostile_git_environment() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(root) = std::env::var_os("VELNOR_GIT_FIXTURE_ROOT") {
        return verify_crate_release_source(
            Path::new(&root),
            &std::env::var("VELNOR_GIT_FIXTURE_SHA")?,
            &test_git(),
        )
        .map_err(Into::into);
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    fs::create_dir(&root)?;
    git(&root, &["init", "-q"])?;
    git(&root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(&root, &["config", "user.email", "alexey@zhokhov.com"])?;
    fs::write(root.join("data"), "approved")?;
    git(&root, &["add", "."])?;
    git(
        &root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    )?;
    let sha = git(&root, &["rev-parse", "HEAD"])?;
    let fake = temp.path().join("git");
    let sentinel = temp.path().join("sentinel");
    fs::write(
        &fake,
        format!("#!/bin/sh\ntouch '{}'\nexit 91\n", sentinel.display()),
    )?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755))?;
    let status = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crate_source_public::source_verification_ignores_hostile_git_environment",
        ])
        .env("VELNOR_GIT_FIXTURE_ROOT", &root)
        .env("VELNOR_GIT_FIXTURE_SHA", &sha)
        .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
        .env("GIT_DIR", temp.path().join("invalid"))
        .env("GIT_INDEX_FILE", temp.path().join("invalid-index"))
        .env("GIT_OBJECT_DIRECTORY", temp.path().join("invalid-objects"))
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_0", &fake)
        .status()?;
    assert!(status.success());
    assert!(!sentinel.exists());
    Ok(())
}

fn planned_inventory(
    root: &Path,
    sha: &str,
    intent: &CrateReleaseIntent,
) -> Result<Vec<velnor_actions_orchestrator::CrateReleaseSourcePackage>, Box<dyn std::error::Error>>
{
    struct EmptyRegistry;
    impl velnor_actions_orchestrator::crate_release_registry::CrateRegistryRead for EmptyRegistry {
        fn get(
            &self,
            _: &str,
        ) -> Result<
            velnor_actions_orchestrator::crate_release_registry::CrateRegistryResponse,
            velnor_actions_orchestrator::OrchestratorError,
        > {
            Err(velnor_actions_orchestrator::OrchestratorError::Inventory {
                problem: "fixture has no registry prerequisites".into(),
            })
        }
    }
    let plan = velnor_actions_orchestrator::crate_release_workspace_plan_with_registry(
        root,
        sha,
        intent,
        &EmptyRegistry,
        &test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    Ok(plan
        .plan
        .selection
        .packages
        .into_iter()
        .map(
            |package| velnor_actions_orchestrator::CrateReleaseSourcePackage {
                name: package.name,
                version: package.version,
                manifest: package.manifest,
            },
        )
        .collect())
}
fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[cfg(unix)]
#[test]
fn journal_inventory_captures_the_approved_tree_and_rejects_changes()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use velnor_actions_orchestrator::{
        crate_release_journal::{JournalFile, JournalFileKind},
        journal_source_inventory,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(root, &["config", "user.email", "alexey@zhokhov.com"])?;
    fs::write(root.join(".gitignore"), "ignored\n")?;
    fs::write(root.join("plain"), "abc")?;
    fs::write(root.join("program"), "abc")?;
    fs::set_permissions(root.join("program"), fs::Permissions::from_mode(0o755))?;
    symlink("plain", root.join("link"))?;
    symlink("link", root.join("nested-link"))?;
    fs::create_dir(root.join("dir"))?;
    fs::write(root.join("dir/file"), "nested")?;
    git(root, &["add", "."])?;
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    let inventory = journal_source_inventory(root, &sha, &test_git())?;
    assert_eq!(inventory.len(), 6);
    let file = |path: &str| inventory.iter().find(|file| file.path == path);
    assert_eq!(
        file("plain"),
        Some(&JournalFile {
            path: "plain".into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            kind: JournalFileKind::Regular { executable: false }
        })
    );
    assert_eq!(
        file("program").map(|file| &file.kind),
        Some(&JournalFileKind::Regular { executable: true })
    );
    assert_eq!(
        file("link"),
        Some(&JournalFile {
            path: "link".into(),
            sha256: "a116c9ed46d6207734a43317d30fd88f52ac8634c37d904bbf4e41d865f90475".into(),
            kind: JournalFileKind::Symlink {
                target: "plain".into()
            }
        })
    );
    assert_eq!(
        file("nested-link").map(|file| &file.kind),
        Some(&JournalFileKind::Symlink {
            target: "link".into()
        })
    );
    assert!(journal_source_inventory(root, &"0".repeat(40), &test_git()).is_err());
    fs::write(root.join("untracked"), "untracked")?;
    assert!(journal_source_inventory(root, &sha, &test_git()).is_err());
    fs::remove_file(root.join("untracked"))?;
    fs::rename(root.join("dir"), root.join("saved-dir"))?;
    symlink("saved-dir", root.join("dir"))?;
    assert!(journal_source_inventory(root, &sha, &test_git()).is_err());
    fs::remove_file(root.join("dir"))?;
    fs::rename(root.join("saved-dir"), root.join("dir"))?;
    fs::write(root.join("ignored"), "ignored but present")?;
    assert!(journal_source_inventory(root, &sha, &test_git()).is_err());
    fs::remove_file(root.join("ignored"))?;
    git(root, &["config", "core.filemode", "false"])?;
    fs::set_permissions(root.join("plain"), fs::Permissions::from_mode(0o755))?;
    assert!(journal_source_inventory(root, &sha, &test_git()).is_err());
    fs::set_permissions(root.join("plain"), fs::Permissions::from_mode(0o644))?;
    git(root, &["rm", "--cached", "plain"])?;
    assert!(journal_source_inventory(root, &sha, &test_git()).is_err());
    Ok(())
}

#[test]
fn approved_source_must_be_exact_clean_repository() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(root, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    git(root, &["config", "core.hooksPath", "/dev/null"])?;
    fs::write(root.join(".gitignore"), "ignored\n")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.1\"\n",
    )?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn demo() {}")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("Cargo.toml", root.join("tracked-link"))?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "fixture",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    verify_crate_release_source(root, &sha, &test_git())?;
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let rust = catalog.version(velnor_actions_mise::PinnedTool::Rust);
    let intent: CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema": 1, "repository": "tailrocks/demo", "default_branch": "main", "environment": "release", "required_workflow_path": ".github/workflows/ci.yml", "trusted_reviewer_id": 139_017, "generator_version":"0.1.72","generator_source_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","generator_manifest_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing","rust_version": rust, "manifest_path": "Cargo.toml", "pr_write_paths": ["CHANGELOG.md", "Cargo.lock", "Cargo.toml"], "registry": "crates-io", "packages": ["demo"], "publish_features": {"demo":[]}, "tag_template": "{{ package }}-v{{ version }}", "recovery": {"package": "demo", "version": "0.1.1", "source_sha": sha}}),
    )?;
    let packages = planned_inventory(root, &sha, &intent)?;
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "demo");
    assert_eq!(packages[0].version, "0.1.1");

    assert!(verify_crate_release_source(root, &"0".repeat(40), &test_git()).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        git(root, &["config", "core.filemode", "false"])?;
        fs::set_permissions(root.join("src/lib.rs"), fs::Permissions::from_mode(0o755))?;
        assert!(verify_crate_release_source(root, &sha, &test_git()).is_err());
        fs::set_permissions(root.join("src/lib.rs"), fs::Permissions::from_mode(0o644))?;
        git(root, &["update-index", "--assume-unchanged", "src/lib.rs"])?;
        fs::write(root.join("src/lib.rs"), "tampered")?;
        assert!(verify_crate_release_source(root, &sha, &test_git()).is_err());
        git(
            root,
            &["update-index", "--no-assume-unchanged", "src/lib.rs"],
        )?;
        git(root, &["checkout", "--", "src/lib.rs"])?;
    }
    fs::write(root.join("Cargo.toml"), "changed")?;
    assert!(verify_crate_release_source(root, &sha, &test_git()).is_err());
    git(root, &["checkout", "--", "Cargo.toml"])?;
    fs::write(root.join("ignored"), "could enter package")?;
    assert!(verify_crate_release_source(root, &sha, &test_git()).is_err());
    fs::remove_file(root.join("ignored"))?;
    verify_crate_release_source(root, &sha, &test_git())?;
    #[cfg(unix)]
    reject_escaping_link(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_verification_does_not_execute_repository_fsmonitor()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    fs::create_dir(&root)?;
    git(&root, &["init", "-q"])?;
    git(&root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(&root, &["config", "user.email", "alexey@zhokhov.com"])?;
    fs::write(root.join("data"), "approved")?;
    fs::write(root.join(".gitattributes"), "data filter=probe\n")?;
    git(&root, &["add", "."])?;
    git(
        &root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    )?;
    let sha = git(&root, &["rev-parse", "HEAD"])?;
    let sentinel = temp.path().join("sentinel");
    let hook = temp.path().join("fsmonitor");
    fs::write(
        &hook,
        format!("#!/bin/sh\ntouch '{}'\n", sentinel.display()),
    )?;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
    git(
        &root,
        &["config", "core.fsmonitor", hook.to_str().ok_or("path")?],
    )?;
    git(
        &root,
        &["config", "filter.probe.clean", hook.to_str().ok_or("path")?],
    )?;
    verify_crate_release_source(&root, &sha, &test_git())?;
    velnor_actions_orchestrator::journal_source_inventory(&root, &sha, &test_git())?;
    assert!(!sentinel.exists());
    git(
        &root,
        &[
            "-c",
            "core.fsmonitor=false",
            "update-index",
            "--force-remove",
            "data",
        ],
    )?;
    assert!(verify_crate_release_source(&root, &sha, &test_git()).is_err());
    assert!(!sentinel.exists());
    git(&root, &["-c", "core.fsmonitor=false", "read-tree", "HEAD"])?;
    fs::write(root.join("new"), "unapproved")?;
    git(&root, &["-c", "core.fsmonitor=false", "add", "new"])?;
    assert!(verify_crate_release_source(&root, &sha, &test_git()).is_err());
    assert!(!sentinel.exists());
    Ok(())
}

#[cfg(unix)]
fn reject_escaping_link(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let outside = tempfile::NamedTempFile::new()?;
    fs::remove_file(root.join("tracked-link"))?;
    std::os::unix::fs::symlink(outside.path(), root.join("tracked-link"))?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "outside link",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let escaped = git(root, &["rev-parse", "HEAD"])?;
    assert!(verify_crate_release_source(root, &escaped, &test_git()).is_err());
    Ok(())
}

#[cfg(unix)]
#[path = "crate_source_public/environment.rs"]
mod environment;
