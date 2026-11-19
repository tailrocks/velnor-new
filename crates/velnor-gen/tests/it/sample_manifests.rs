// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use velnor_gen::io::read_checkout;

type TestResult = Result<(), Box<dyn Error>>;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn git(directory: &std::path::Path, args: &[&str]) -> TestResult {
    let status = Command::new(crate::setup_action::program("git"))
        .args(args)
        .current_dir(directory)
        .status()?;
    assert!(status.success(), "git {args:?}");
    Ok(())
}

#[test]
fn a_recorded_sample_manifest_in_a_real_repository_is_not_a_workspace_root() -> TestResult {
    let repo = Scratch(std::env::temp_dir().join(format!(
        "velnor-gen-sample-manifests-{}",
        std::process::id()
    )));
    fs::remove_dir_all(&repo.0).ok();
    let root = &repo.0;
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("src/lib.rs"), "")?;
    // A recorded sample view holds a manifest whose crate has no sources.
    let view = root.join("generator/samples/x/view");
    fs::create_dir_all(&view)?;
    fs::write(
        view.join("Cargo.toml"),
        "[package]\nname = \"recorded\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    git(root, &["init", "--quiet"])?;
    git(root, &["add", "--all"])?;
    let checkout = read_checkout(root);
    assert!(
        checkout.is_ok(),
        "a sample manifest was taken for a workspace root: {:?}",
        checkout.err()
    );
    Ok(())
}
