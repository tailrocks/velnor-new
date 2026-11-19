// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Test-only cache of freshly bootstrapped journals.
//!
//! Creating a journal from nothing replays the whole schema (hundreds of
//! tables and triggers, one DDL statement each), which costs tens of seconds
//! per test in a debug build. Every nextest test runs in its own process, so a
//! per-process cache cannot help. The first process to need a given schema
//! (`key`) publishes the closed journal files next to the test executable;
//! later tests copy them and then run the normal open path against a journal
//! that is already at that schema.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Sibling files a closed journal can leave behind, as suffixes of its name.
const SUFFIXES: [&str; 3] = ["", "-wal", "-shm"];

fn root() -> io::Result<(PathBuf, String)> {
    let exe = std::env::current_exe()?;
    let meta = fs::metadata(&exe)?;
    let nanos = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut name: OsString = exe
        .file_name()
        .ok_or_else(|| io::Error::other("test executable has no file name"))?
        .to_owned();
    name.push(".journal-templates");
    Ok((exe.with_file_name(name), format!("{}-{nanos}", meta.len())))
}

fn base(path: &Path) -> io::Result<(&Path, &str)> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("journal path has no parent"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("journal path is not UTF-8"))?;
    Ok((parent, name))
}

/// What `seed` found at the journal path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Seed {
    /// The journal already existed, so it holds someone's state: leave it alone.
    Existing,
    /// The journal was copied from the template for its key.
    Copied,
    /// No journal and no template: bootstrap one and `publish` it.
    Missing,
}

impl Seed {
    /// Whether the journal is already at the schema for the key.
    pub(super) const fn was_copied(self) -> bool {
        matches!(self, Self::Copied)
    }

    /// Whether the journal was created by this open and is still pristine.
    pub(super) const fn is_pristine(self) -> bool {
        matches!(self, Self::Missing)
    }
}

/// Copies the published journal for `key` to `path` when `path` does not
/// exist yet; any failure leaves `path` untouched so the caller falls back to
/// bootstrapping.
pub(super) fn seed(path: &Path, key: &str) -> Seed {
    if path.exists() {
        return Seed::Existing;
    }
    let attempt = || -> io::Result<bool> {
        let (root, stamp) = root()?;
        let template = root.join(stamp).join(key);
        if !template.is_dir() {
            return Ok(false);
        }
        let (parent, name) = base(path)?;
        for suffix in SUFFIXES {
            let source = template.join(format!("db{suffix}"));
            if source.exists() {
                fs::copy(&source, parent.join(format!("{name}{suffix}")))?;
            }
        }
        Ok(true)
    };
    if let Ok(true) = attempt() {
        return Seed::Copied;
    }
    if let Ok((parent, name)) = base(path) {
        for suffix in SUFFIXES {
            fs::remove_file(parent.join(format!("{name}{suffix}"))).ok();
        }
    }
    Seed::Missing
}

/// Publishes the just-bootstrapped, closed, still pristine journal at `path`
/// as the template for `key`. Best effort: a failure only costs later tests the speed-up.
pub(super) fn publish(path: &Path, key: &str) {
    try_publish(path, key).ok();
}

fn try_publish(path: &Path, key: &str) -> io::Result<()> {
    let (root, stamp) = root()?;
    let (parent, name) = base(path)?;
    let stamp_dir = root.join(&stamp);
    fs::create_dir_all(&stamp_dir)?;
    let staging = stamp_dir.join(format!(".{key}.{}", std::process::id()));
    fs::remove_dir_all(&staging).ok();
    fs::create_dir(&staging)?;
    let copied = SUFFIXES.iter().try_for_each(|suffix| {
        let source = parent.join(format!("{name}{suffix}"));
        if source.exists() {
            fs::copy(&source, staging.join(format!("db{suffix}"))).map(drop)
        } else {
            Ok(())
        }
    });
    // Renaming a directory is atomic: readers see all of it or none of it.
    let published = copied.and_then(|()| fs::rename(&staging, stamp_dir.join(key)));
    if published.is_err() {
        fs::remove_dir_all(&staging).ok();
    }
    // Drop templates of earlier builds of this executable.
    for entry in fs::read_dir(&root)?.flatten() {
        if entry.file_name().to_str() != Some(stamp.as_str()) {
            fs::remove_dir_all(entry.path()).ok();
        }
    }
    Ok(())
}

mod tests {
    use super::super::Journal;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    async fn schema(journal: &Journal) -> TestResult<(i64, Vec<(String, String)>)> {
        let connection = journal.connection().await?;
        let mut rows = connection.query("PRAGMA user_version", ()).await?;
        let version: i64 = rows.next().await?.ok_or("user_version")?.get(0)?;
        let mut rows = connection
            .query(
                "SELECT name, COALESCE(sql, '') FROM sqlite_schema ORDER BY name",
                (),
            )
            .await?;
        let mut objects = Vec::new();
        while let Some(row) = rows.next().await? {
            objects.push((row.get(0)?, row.get(1)?));
        }
        Ok((version, objects))
    }

    /// Opening a current journal re-installs its triggers in canonical form, so
    /// a first bootstrap is compared after one reopen, as every template user
    /// sees it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_journal_copied_from_the_template_matches_a_freshly_bootstrapped_one() -> TestResult {
        let directory = tempfile::tempdir()?;
        let fresh = directory.path().join("fresh.db");
        drop(Journal::open_bootstrapped(&fresh).await?);
        let expected = schema(&Journal::open_bootstrapped(&fresh).await?).await?;
        assert_eq!(expected.0, Journal::current_schema_version_for_test());
        assert!(
            expected.1.len() > 100,
            "the schema holds hundreds of objects"
        );
        for name in ["first.db", "second.db"] {
            let journal = Journal::open(&directory.path().join(name)).await?;
            assert_eq!(schema(&journal).await?, expected, "{name}");
        }
        Ok(())
    }
}
