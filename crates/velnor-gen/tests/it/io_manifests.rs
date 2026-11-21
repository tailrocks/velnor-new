// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use velnor_gen::io::{Error as IoError, view_of};
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
        fs::create_dir_all(base.join(".velnor"))?;
        fs::write(base.join(".velnor/rust-ci.toml"), "exclude = [\"ex\"]\n")?;
        fs::create_dir_all(base.join("ex"))?;
        Ok(Self(fs::canonicalize(base)?))
    }

    fn tracked(manifest: &RepoPath) -> Result<BTreeSet<RepoPath>, Box<dyn Error>> {
        Ok(BTreeSet::from([
            RepoPath::new(".velnor/rust-ci.toml")?,
            manifest.clone(),
        ]))
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
    let view = view_of(
        &readable.0,
        Checkout::tracked(&manifest)?,
        BTreeSet::new(),
        BTreeSet::new(),
        false,
    )?
    .0;
    assert!(view.files.contains_key(&manifest));

    let dangling = Checkout::new("dangling")?;
    symlink(
        Path::new("missing/Cargo.toml"),
        dangling.0.join(manifest.as_str()),
    )?;
    let result = view_of(
        &dangling.0,
        Checkout::tracked(&manifest)?,
        BTreeSet::new(),
        BTreeSet::new(),
        false,
    );
    assert!(
        matches!(result, Err(IoError::NotRegular(ref path)) if *path == manifest),
        "{:?}",
        result.map(|_| ())
    );
    Ok(())
}

#[test]
fn an_unreadable_workspace_member_lock_fails_the_checkout() -> TestResult {
    let manifest = RepoPath::new("app/Cargo.toml")?;
    let checkout = Checkout::new("member")?;
    fs::create_dir_all(checkout.0.join("app"))?;
    fs::write(checkout.0.join(".velnor/rust-ci.toml"), "exclude = []\n")?;
    fs::write(checkout.0.join(manifest.as_str()), "[package]\n")?;
    symlink(
        Path::new("missing/Cargo.lock"),
        checkout.0.join("app/Cargo.lock"),
    )?;
    let result = view_of(
        &checkout.0,
        Checkout::tracked(&manifest)?,
        BTreeSet::new(),
        BTreeSet::new(),
        false,
    );
    let lock = RepoPath::new("app/Cargo.lock")?;
    assert!(
        matches!(result, Err(IoError::NotRegular(ref path)) if *path == lock),
        "{:?}",
        result.map(|_| ())
    );
    Ok(())
}
