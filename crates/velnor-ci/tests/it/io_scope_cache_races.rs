// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::error::CiError;
use velnor_ci::io::{ScopeCacheIo, restore_inputs_batch_with};

use crate::io::Repo;
use crate::io_scope_cache::prepared;

#[derive(Clone, Copy)]
enum Mutation {
    None,
    ReadLock,
    ObserveFile,
    ObserveIndex,
    PublishFile,
    PublishIndex,
}

struct FileSystem {
    root: PathBuf,
    mutation: Mutation,
    published: Cell<bool>,
    observed: Cell<bool>,
}

impl ScopeCacheIo for FileSystem {
    fn metadata(&self, path: &Path) -> Result<fs::Metadata, CiError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|error| CiError::new(error.to_string()))?;
        if path == self.root.join("Cargo.toml") {
            match self.mutation {
                Mutation::ObserveFile => self.mutate(Mutation::PublishFile)?,
                Mutation::ObserveIndex => self.mutate(Mutation::PublishIndex)?,
                _ => {}
            }
        }
        Ok(metadata)
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, CiError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(CiError::new(error.to_string())),
        };
        if path.ends_with(".velnor/rust-inputs.lock") {
            self.observed.set(true);
            if matches!(self.mutation, Mutation::ReadLock) {
                fs::write(path, "data.bin\tapp\n")
                    .map_err(|error| CiError::new(error.to_string()))?;
            }
        }
        Ok(bytes)
    }

    fn publish(&self, path: &Path, text: &str) -> Result<(), CiError> {
        self.published.set(true);
        self.mutate(self.mutation)?;
        fs::write(path, text).map_err(|error| CiError::new(error.to_string()))
    }
}

fn refresh(mutation: Mutation) {
    let (repo, cache) = prepared();
    let baseline = FileSystem {
        root: fs::canonicalize(repo.root()).unwrap_or_else(|error| unreachable!("{error}")),
        mutation: Mutation::None,
        published: Cell::new(false),
        observed: Cell::new(false),
    };
    restore_inputs_batch_with(
        repo.root(),
        &[PathBuf::from("."), PathBuf::from("custom-target")],
        Some(&cache),
        &baseline,
    )
    .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(cache.is_file());
    let manifest =
        fs::read(repo.root().join("Cargo.toml")).unwrap_or_else(|error| unreachable!("{error}"));
    let source =
        fs::read(repo.root().join("src/lib.rs")).unwrap_or_else(|error| unreachable!("{error}"));
    let checkout_time = UNIX_EPOCH + Duration::from_secs(1_700_000_123);
    repo.set_mtime("src/lib.rs", checkout_time);
    let io = FileSystem {
        root: fs::canonicalize(repo.root()).unwrap_or_else(|error| unreachable!("{error}")),
        mutation,
        published: Cell::new(false),
        observed: Cell::new(false),
    };
    let result = restore_inputs_batch_with(
        repo.root(),
        &[PathBuf::from("."), PathBuf::from("custom-target")],
        Some(&cache),
        &io,
    );
    assert!(io.observed.get());
    assert_eq!(
        io.published.get(),
        matches!(
            mutation,
            Mutation::None | Mutation::PublishFile | Mutation::PublishIndex
        )
    );
    if matches!(mutation, Mutation::None) {
        assert!(result.is_ok(), "{result:?}");
        assert!(cache.is_file());
    } else {
        assert!(result.is_err(), "{result:?}");
        assert!(!cache.exists());
        assert_eq!(repo.mtime("src/lib.rs"), checkout_time);
        assert_eq!(
            fs::read(repo.root().join("src/lib.rs")).unwrap_or_default(),
            source
        );
    }
    match mutation {
        Mutation::ReadLock => {
            fs::write(
                repo.root().join(".velnor/rust-inputs.lock"),
                "data.bin\tone\n",
            )
            .unwrap_or_else(|error| unreachable!("{error}"));
        }
        Mutation::PublishFile | Mutation::ObserveFile => {
            fs::write(repo.root().join("Cargo.toml"), manifest)
                .unwrap_or_else(|error| unreachable!("{error}"));
        }
        Mutation::PublishIndex | Mutation::ObserveIndex => {
            repo.git(&["reset", "--quiet", "HEAD", "--", "new-input.txt"]);
            fs::remove_file(repo.root().join("new-input.txt"))
                .unwrap_or_else(|error| unreachable!("{error}"));
        }
        Mutation::None => {}
    }
    save_after_refresh(&repo, &cache, matches!(mutation, Mutation::None));
}

#[test]
fn a_lock_changed_between_consuming_it_and_detection_cannot_refresh_a_cached_scope() {
    refresh(Mutation::ReadLock);
}

#[test]
fn a_file_changed_during_publication_cannot_refresh_a_cached_scope() {
    refresh(Mutation::PublishFile);
}

#[test]
fn an_index_changed_during_publication_cannot_refresh_a_cached_scope() {
    refresh(Mutation::PublishIndex);
}

#[test]
fn unchanged_observations_can_refresh_a_cached_scope_through_the_same_io_boundary() {
    refresh(Mutation::None);
}

impl FileSystem {
    fn mutate(&self, mutation: Mutation) -> Result<(), CiError> {
        match mutation {
            Mutation::PublishFile => {
                let manifest = self.root.join("Cargo.toml");
                let mut bytes =
                    fs::read(&manifest).map_err(|error| CiError::new(error.to_string()))?;
                bytes.push(b'\n');
                fs::write(manifest, bytes).map_err(|error| CiError::new(error.to_string()))?;
            }
            Mutation::PublishIndex => {
                fs::write(self.root.join("new-input.txt"), "new\n")
                    .map_err(|error| CiError::new(error.to_string()))?;
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&self.root)
                    .args(["add", "new-input.txt"])
                    .status()
                    .map_err(|error| CiError::new(error.to_string()))?;
                if !status.success() {
                    return Err(CiError::new("cannot add input"));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[test]
fn a_file_changed_before_fingerprinting_cannot_replace_the_consumed_bytes() {
    refresh(Mutation::ObserveFile);
}

#[test]
fn an_index_changed_before_fingerprinting_cannot_replace_the_consumed_index() {
    refresh(Mutation::ObserveIndex);
}

fn save_after_refresh(repo: &Repo, cache: &Path, successful: bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env(
            "GITHUB_JOB",
            std::env::var("GITHUB_JOB").unwrap_or_default(),
        )
        .args([
            "inputs",
            "save",
            "--target-dir",
            "custom-target",
            "--scope-cache",
        ])
        .arg(cache)
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert_eq!(output.status.success(), successful, "{output:?}");
    if !successful {
        assert!(
            !repo
                .root()
                .join("custom-target/.velnor/inputs.tsv")
                .exists()
        );
    }
}
