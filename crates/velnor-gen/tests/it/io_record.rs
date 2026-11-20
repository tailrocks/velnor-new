// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

use velnor_gen::io::{
    Error as IoError, Recording, SAMPLE_REPOSITORY, SampleWriteOutcome, recording_of,
    relocate_metadata, write_sample,
};
use velnor_model::RepoPath;

use super::io::{Scratch, TestResult, write};

fn files(entries: &[(&str, &str)]) -> Result<BTreeMap<RepoPath, Vec<u8>>, Box<dyn Error>> {
    entries
        .iter()
        .map(|(path, text)| Ok((RepoPath::new(*path)?, text.as_bytes().to_vec())))
        .collect()
}

#[test]
fn recording_a_sample_replaces_the_recorded_inputs_and_keeps_the_expected_files() -> TestResult {
    let scratch = Scratch::new("record")?;
    let samples = scratch.repo().join("generator/samples");
    let sample = scratch.repo().join("generator/samples/demo");
    let expected = sample.join("expected");
    write(
        &sample.join("expected/.github/workflows/rust-ci.yml"),
        "kept\n",
    )?;
    symlink(".github/workflows/rust-ci.yml", expected.join("link"))?;
    write(&sample.join("view/old/Cargo.toml"), "")?;
    write(&sample.join("cargo-metadata/old.json"), "{}")?;
    fs::set_permissions(&expected, fs::Permissions::from_mode(0o555))?;
    let result = write_sample(
        &scratch.repo(),
        "demo",
        &Recording {
            view: files(&[("mise.toml", "[tools]\n")])?,
            manifests: vec![
                RepoPath::new("Cargo.toml")?,
                RepoPath::new("lab/Cargo.toml")?,
            ],
            symlinks: vec![RepoPath::new(".cargo")?],
            metadata: files(&[("lab/Cargo.toml", "{}\n")])?,
        },
    );
    fs::set_permissions(&expected, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(&samples)? {
        let expected = entry?.path().join("previous/expected");
        if expected.exists() {
            fs::set_permissions(expected, fs::Permissions::from_mode(0o755))?;
        }
    }
    assert_eq!(result?, SampleWriteOutcome::Recorded);
    for (path, bytes) in [
        ("manifests.txt", "Cargo.toml\nlab/Cargo.toml\n"),
        ("symlinks.txt", ".cargo\n"),
        ("view/mise.toml", "[tools]\n"),
        ("cargo-metadata/lab/Cargo.toml.json", "{}\n"),
        ("expected/.github/workflows/rust-ci.yml", "kept\n"),
    ] {
        assert_eq!(fs::read_to_string(sample.join(path))?, bytes, "{path}");
    }
    assert!(!sample.join("view/old").exists());
    assert!(!sample.join("cargo-metadata/old.json").exists());
    assert_eq!(
        fs::read_link(sample.join("expected/link"))?,
        Path::new(".github/workflows/rust-ci.yml")
    );
    assert_eq!(fs::read_dir(&samples)?.count(), 1);
    Ok(())
}

#[test]
fn recording_a_sample_preserves_symlinks_from_the_repository_view() -> TestResult {
    let scratch = Scratch::new("record-symlink")?;
    let sample = crate::golden::samples().join("single-crate");
    let mut view = crate::golden::load(&sample)?;
    view.symlinks.insert(RepoPath::new("README.md")?);
    let metadata = BTreeMap::from([(
        RepoPath::new("Cargo.toml")?,
        fs::read(sample.join("cargo-metadata/Cargo.toml.json"))?,
    )]);
    let recording = recording_of(
        Path::new(SAMPLE_REPOSITORY),
        view,
        metadata,
        &mut Vec::new(),
    )?;
    write_sample(&scratch.repo(), "demo", &recording)?;
    let recorded = crate::golden::load(&scratch.repo().join("generator/samples/demo"))?;
    assert!(recorded.symlinks.contains(&RepoPath::new("README.md")?));
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn a_published_sample_reports_failure_to_remove_its_previous_version() -> TestResult {
    let scratch = WorktreeScratch::new()?;
    let sample = scratch.repo().join("generator/samples/demo");
    let obsolete = sample.join("view/obsolete.txt");
    write(&obsolete, "old\n")?;
    let immutable = ImmutableTree::new(scratch.repo().join("generator"), &obsolete);
    let result = write_sample(
        &scratch.repo(),
        "demo",
        &Recording {
            view: files(&[("mise.toml", "[tools]\n")])?,
            ..Recording::default()
        },
    )?;
    let SampleWriteOutcome::RecordedWithCleanupFailure {
        staging_path,
        error,
    } = result
    else {
        return Err("published sample did not report cleanup failure".into());
    };
    assert_ne!(error, "");
    assert!(staging_path.is_dir());
    assert_eq!(
        staging_path.parent(),
        Some(scratch.repo().join("generator/samples").as_path())
    );

    assert_eq!(
        fs::read_to_string(sample.join("view/mise.toml"))?,
        "[tools]\n"
    );
    assert!(!sample.join("view/obsolete.txt").exists());
    assert_eq!(
        fs::read_to_string(staging_path.join("previous/view/obsolete.txt"))?,
        "old\n"
    );
    drop(immutable);
    Ok(())
}

#[cfg(target_os = "macos")]
struct ImmutableTree(std::path::PathBuf);

#[cfg(target_os = "macos")]
impl ImmutableTree {
    fn new(root: std::path::PathBuf, file: &Path) -> Self {
        let status = Command::new("/usr/bin/chflags")
            .arg("uchg")
            .arg(file)
            .status();
        assert!(
            status.as_ref().is_ok_and(std::process::ExitStatus::success),
            "{status:?}"
        );
        Self(root)
    }
}

#[cfg(target_os = "macos")]
impl Drop for ImmutableTree {
    fn drop(&mut self) {
        Command::new("/usr/bin/chflags")
            .arg("-R")
            .arg("nouchg")
            .arg(&self.0)
            .status()
            .ok();
    }
}

#[cfg(target_os = "macos")]
struct WorktreeScratch(std::path::PathBuf);

#[cfg(target_os = "macos")]
impl WorktreeScratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::current_dir()?
            .join(".tmp-cleanup-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn repo(&self) -> std::path::PathBuf {
        self.0.join("repo")
    }
}

#[cfg(target_os = "macos")]
impl Drop for WorktreeScratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

mod metadata;

