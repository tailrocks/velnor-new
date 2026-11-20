// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::process::{Command, Output};

use crate::release_symlink::{Scratch, git};

type TestResult = Result<(), Box<dyn Error>>;

const CONFIGS: &[&str] = &[
    ".velnor/rust-ci.toml",
    ".velnor/release.toml",
    ".velnor/workflows.toml",
];

fn repo(name: &str) -> Result<Scratch, Box<dyn Error>> {
    let repo = Scratch(std::env::temp_dir().join(format!(
        "velnor-gen-untracked-{name}-{}",
        std::process::id()
    )));
    fs::remove_dir_all(&repo.0).ok();
    fs::create_dir_all(repo.0.join(".velnor"))?;
    git(&repo.0, &["init", "--quiet"])?;
    Ok(repo)
}

fn velnor_gen(repo: &Scratch, command: &str) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", command])
        .current_dir(&repo.0)
        .env("PATH", "/usr/bin:/bin")
        .output()?)
}

#[test]
fn an_untracked_config_is_refused_by_generate_and_check_and_names_the_file() -> TestResult {
    for path in CONFIGS.iter().copied() {
        for command in ["generate", "check"] {
            let repo = repo(&format!(
                "refused-{command}-{}",
                path.replace(['/', '.'], "_")
            ))?;
            fs::write(repo.0.join(path), "runner = \"self-hosted\"\n")?;
            let output = velnor_gen(&repo, command)?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{command} {path}: {stderr}");
            assert!(stderr.contains(path), "{command} {path}: {stderr}");
            assert!(stderr.contains(&format!("git add {path}")), "{stderr}");
            assert!(
                !repo.0.join(".github/workflows/rust-ci.yml").exists(),
                "{command} {path} wrote workflows before refusing"
            );
        }
    }
    Ok(())
}

#[test]
fn a_git_ignored_config_is_refused_as_untracked() -> TestResult {
    let repo = repo("ignored")?;
    fs::write(repo.0.join(".gitignore"), ".velnor/\n")?;
    fs::write(repo.0.join(".velnor/rust-ci.toml"), "runner = \"x\"\n")?;
    let output = velnor_gen(&repo, "check")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(".velnor/rust-ci.toml"), "{stderr}");
    Ok(())
}

#[test]
fn a_tracked_config_is_not_refused_as_untracked() -> TestResult {
    for path in CONFIGS.iter().copied() {
        let repo = repo(&format!("tracked-{}", path.replace(['/', '.'], "_")))?;
        fs::write(repo.0.join(path), "runner = \"self-hosted\"\n")?;
        git(&repo.0, &["add", path])?;
        let output = velnor_gen(&repo, "check")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("git add"), "{path}: {stderr}");
    }
    Ok(())
}

#[test]
fn a_repository_without_configs_is_not_refused() -> TestResult {
    let repo = repo("none")?;
    let output = velnor_gen(&repo, "check")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("git add"), "{stderr}");
    Ok(())
}
