// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::workflow_configuration_fixture::{git, make_repo};
use std::{error::Error, fs};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

#[test]
fn release_generation_requires_tracked_manifests_without_changing_the_source()
-> Result<(), Box<dyn Error>> {
    let config = "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n";
    let repo = make_repo(config)?;
    let root = repo.path();
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/demo.git",
        ],
        root,
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\nexclude = [\"helper\"]\n[dependencies]\nhelper = { path = \"helper\", version = \"0.1.0\" }\n",
    )?;
    fs::create_dir_all(root.join("helper/src"))?;
    let manifest = root.join("helper/Cargo.toml");
    fs::write(
        &manifest,
        "[package]\nname = \"helper\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("helper/src/lib.rs"), "pub fn helper() {}\n")?;
    git(&["add", "."], root)?;
    let before = fs::read(&manifest)?;
    let initial_status = git_status(root)?;
    render_staged_tree(&prepare(root)?)?;
    assert_eq!(git_status(root)?, initial_status);
    assert_eq!(fs::read(&manifest)?, before);
    git(&["rm", "--cached", "helper/Cargo.toml"], root)?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    assert_eq!(fs::read(&manifest)?, before);
    git(&["add", "helper/Cargo.toml"], root)?;
    fs::remove_file(&manifest)?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    fs::write(&manifest, &before)?;
    fs::write(&manifest, b"[invalid")?;
    git(&["add", "helper/Cargo.toml"], root)?;
    fs::write(&manifest, &before)?;
    assert!(render_staged_tree(&prepare(root)?).is_ok());
    git(&["rm", "--cached", "Cargo.toml"], root)?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nhelper = { path = \"helper\", version = \"0.1.0\" }\n",
    )?;
    git(&["add", "Cargo.toml", "helper/Cargo.toml"], root)?;
    git(&["rm", "--cached", "helper/Cargo.toml"], root)?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    reject_ignored_and_escaping(root, &manifest, &before)?;
    Ok(())
}

fn reject_ignored_and_escaping(
    root: &std::path::Path,
    manifest: &std::path::Path,
    before: &[u8],
) -> Result<(), Box<dyn Error>> {
    fs::write(root.join(".gitignore"), "helper/Cargo.toml\n")?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir()?;
        let outside_manifest = outside.path().join("Cargo.toml");
        fs::write(&outside_manifest, before)?;
        fs::remove_file(manifest)?;
        std::os::unix::fs::symlink(&outside_manifest, manifest)?;
        git(&["add", "-f", "helper/Cargo.toml"], root)?;
        assert!(
            prepare(root)
                .and_then(|prep| render_staged_tree(&prep))
                .is_err()
        );
    }
    Ok(())
}

fn git_status(root: &std::path::Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let result = super::workflow_configuration_fixture::git_fixture::command(root)?
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored",
        ])
        .output()?;
    if !result.status.success() {
        return Err("git status failed".into());
    }
    Ok(result.stdout)
}

#[cfg(unix)]
#[test]
fn excluded_path_dependency_manifest_cannot_escape_tracked_source() -> Result<(), Box<dyn Error>> {
    let config = "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n";
    let repo = make_repo(config)?;
    let root = repo.path();
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/demo.git",
        ],
        root,
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n[workspace]\nexclude=['helper']\n[dependencies]\nhelper={path='helper',version='0.1.0'}\n",
    )?;
    fs::create_dir_all(root.join("helper/src"))?;
    let manifest = root.join("helper/Cargo.toml");
    let contents = "[package]\nname='helper'\nversion='0.1.0'\nedition='2021'\n";
    fs::write(&manifest, contents)?;
    fs::write(root.join("helper/src/lib.rs"), "pub fn f() {}\n")?;
    git(&["add", "."], root)?;
    assert!(render_staged_tree(&prepare(root)?).is_ok());
    let outside = tempfile::tempdir()?;
    let outside_manifest = outside.path().join("Cargo.toml");
    fs::write(&outside_manifest, contents)?;
    fs::remove_file(&manifest)?;
    std::os::unix::fs::symlink(&outside_manifest, &manifest)?;
    git(&["add", "helper/Cargo.toml"], root)?;
    let before = git_status(root)?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    assert_eq!(git_status(root)?, before);
    fs::remove_file(&manifest)?;
    let untracked = root.join("untracked.toml");
    fs::write(&untracked, contents)?;
    std::os::unix::fs::symlink(&untracked, &manifest)?;
    git(&["add", "helper/Cargo.toml"], root)?;
    assert!(
        prepare(root)
            .and_then(|prep| render_staged_tree(&prep))
            .is_err()
    );
    Ok(())
}
