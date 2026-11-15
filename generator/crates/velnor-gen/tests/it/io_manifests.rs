// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_gen::io::{Error as IoError, read_checkout};
use velnor_model::RepoPath;

type TestResult = Result<(), Box<dyn Error>>;

struct Checkout(PathBuf);

impl Checkout {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let base = std::env::temp_dir().join(format!(
            "velnor-gen-io-manifests-{}-{name}",
            std::process::id()
        ));
        if base.exists() {
            fs::remove_dir_all(&base)?;
        }
        fs::create_dir_all(&base)?;
        let checkout = Self(fs::canonicalize(base)?);
        checkout.git(&["init", "--quiet"])?;
        fs::create_dir_all(checkout.0.join(".velnor"))?;
        fs::write(
            checkout.0.join(".velnor/rust-ci.toml"),
            "exclude = [\"ex\"]\n",
        )?;
        fs::create_dir_all(checkout.0.join("ex"))?;
        Ok(checkout)
    }

    fn git(&self, args: &[&str]) -> TestResult {
        let status = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .status()?;
        status.success().then_some(()).ok_or("git failed".into())
    }

    fn track(&self) -> TestResult {
        self.git(&["add", "--force", "--all"])
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn a_tracked_manifest_that_cannot_be_read_fails_the_checkout() -> TestResult {
    let manifest = RepoPath::new("ex/Cargo.toml")?;
    let readable = Checkout::new("readable")?;
    fs::write(readable.0.join(manifest.as_str()), "[package]\n")?;
    readable.track()?;
    let view = read_checkout(&readable.0)?.view;
    assert!(view.files.contains_key(&manifest));

    let dangling = Checkout::new("dangling")?;
    symlink(
        Path::new("missing/Cargo.toml"),
        dangling.0.join(manifest.as_str()),
    )?;
    dangling.track()?;
    let result = read_checkout(&dangling.0);
    assert!(
        matches!(result, Err(IoError::NotRegular(ref path)) if *path == manifest),
        "{:?}",
        result.map(|_| ())
    );
    Ok(())
}
