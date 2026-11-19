// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;

use crate::golden::samples;
use crate::release_fixture::{APP, release_view};

type TestResult = Result<(), Box<dyn Error>>;

const PROVIDER: &str = "[registry]\ncredential-provider = [\"./steal\"]\n";

pub(crate) struct Scratch(pub(crate) PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> TestResult {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub(crate) fn git(directory: &Path, args: &[&str]) -> TestResult {
    let status = Command::new(crate::setup_action::program("git"))
        .args(args)
        .current_dir(directory)
        .status()?;
    assert!(status.success(), "git {args:?}");
    Ok(())
}

pub(crate) fn run(repo: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "release", "ruleset"])
        .current_dir(repo)
        .env("PATH", "/usr/bin:/bin")
        .output()?)
}

#[test]
fn a_committed_cargo_symlink_hiding_a_credential_provider_is_refused_in_a_real_repository()
-> TestResult {
    let repo = Scratch(
        std::env::temp_dir().join(format!("velnor-gen-release-symlink-{}", std::process::id())),
    );
    fs::remove_dir_all(&repo.0).ok();
    copy_tree(&samples().join("release-two-roots/view"), &repo.0)?;
    fs::create_dir_all(repo.0.join("cfg"))?;
    fs::write(repo.0.join("cfg/config.toml"), PROVIDER)?;
    git(&repo.0, &["init", "--quiet"])?;
    git(&repo.0, &["add", "--all"])?;
    assert!(run(&repo.0)?.status.success());
    symlink("cfg", repo.0.join(".cargo"))?;
    git(&repo.0, &["add", "--all"])?;
    let output = run(&repo.0)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(".cargo: a tracked symbolic link"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn rendering_a_release_refuses_a_tracked_symlink_on_a_root_s_cargo_config_path() -> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    assert!(expected_files(&view).is_ok());
    view.symlinks.insert(RepoPath::new(".cargo")?);
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Release(_))
    ));
    Ok(())
}
