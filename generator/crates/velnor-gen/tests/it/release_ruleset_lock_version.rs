// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_release::ReleaseError;

use crate::io::write;
use crate::release_symlink::{Scratch, git};

type TestResult = Result<(), Box<dyn Error>>;

const LOCKED: &str = "1.2.3-alpha.lock";
const RELEASE_TOML: &str = "roots = [\"member\"]\n";
const RELEASE_PLZ: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\ngit_only = true\n\n[[package]]\nname = \"app\"\n\n[[package]]\nname = \"out\"\n";
const MEMBER: &str = "[package]\nname = \"app\"\nversion.workspace = true\nedition = \"2024\"\n";

fn workspace(version: &str) -> String {
    format!(
        "[workspace]\nmembers = [\"member\", \"outside\"]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"{version}\"\n"
    )
}

fn outside(version: &str) -> String {
    format!("[package]\nname = \"out\"\nversion = \"{version}\"\nedition = \"2024\"\n")
}

fn repository(
    name: &str,
    workspace_version: &str,
    outside_version: &str,
) -> Result<Scratch, Box<dyn Error>> {
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "velnor-gen-ruleset-lock-{}-{name}",
        std::process::id()
    )));
    fs::remove_dir_all(&scratch.0).ok();
    let root: &PathBuf = &scratch.0;
    write(&root.join("Cargo.toml"), &workspace(workspace_version))?;
    write(&root.join("member/Cargo.toml"), MEMBER)?;
    write(&root.join("member/src/lib.rs"), "")?;
    write(&root.join("outside/Cargo.toml"), &outside(outside_version))?;
    write(&root.join("outside/src/lib.rs"), "")?;
    write(&root.join(".velnor/release.toml"), RELEASE_TOML)?;
    write(&root.join("member/release-plz.toml"), RELEASE_PLZ)?;
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/release-two-roots/view");
    for name in ["mise.toml", "mise.lock"] {
        fs::copy(sample.join(name), root.join(name))?;
    }
    git(root, &["init", "--quiet"])?;
    git(root, &["add", "--all"])?;
    Ok(scratch)
}

fn ruleset(root: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    let cargo = Path::new(env!("CARGO"));
    let directory = cargo.parent().ok_or("cargo executable has no parent")?;
    let search_path = std::env::join_paths([directory, Path::new("/usr/bin"), Path::new("/bin")])?;
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "release", "ruleset"])
        .current_dir(root)
        .env("PATH", search_path)
        .output()?)
}

fn refused(root: &Path, manifest: &str) -> TestResult {
    let output = ruleset(root)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let seen = format!("stdout: {stdout}\nstderr: {stderr}");
    let refusal =
        ReleaseError::VersionEndsInLock(manifest.to_owned(), LOCKED.to_owned()).to_string();
    assert_eq!(output.status.code(), Some(2), "{seen}");
    assert!(stderr.contains(&refusal), "{seen}");
    Ok(())
}

#[test]
fn the_ruleset_command_refuses_a_lock_version_of_a_released_member() -> TestResult {
    let inherited = repository("inherited", LOCKED, "1.2.3")?;
    refused(&inherited.0, "member/Cargo.toml")?;
    let outside = repository("outside", "1.2.3", LOCKED)?;
    refused(&outside.0, "outside/Cargo.toml")?;
    Ok(())
}
