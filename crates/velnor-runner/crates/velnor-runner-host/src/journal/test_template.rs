// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Seed {
    Existing,
    Copied,
    Missing,
}

impl Seed {
    pub(super) const fn was_copied(self) -> bool {
        matches!(self, Self::Copied)
    }

    pub(super) const fn is_pristine(self) -> bool {
        matches!(self, Self::Missing)
    }
}

pub(super) fn seed(path: &Path, key: &str) -> Seed {
    if path.exists() {
        return Seed::Existing;
    }
    let started = std::cell::Cell::new(false);
    let attempt = || -> io::Result<bool> {
        let (root, stamp) = root()?;
        let template = root.join(stamp).join(key);
        if !template.is_dir() {
            return Ok(false);
        }
        let (parent, name) = base(path)?;
        started.set(true);
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
    if !started.get() {
        return Seed::Missing;
    }
    if let Ok((parent, name)) = base(path) {
        for suffix in SUFFIXES {
            fs::remove_file(parent.join(format!("{name}{suffix}"))).ok();
        }
    }
    Seed::Missing
}

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
    let published = copied.and_then(|()| fs::rename(&staging, stamp_dir.join(key)));
    if published.is_err() {
        fs::remove_dir_all(&staging).ok();
    }
    for entry in fs::read_dir(&root)?.flatten() {
        if entry.file_name().to_str() != Some(stamp.as_str()) {
            fs::remove_dir_all(entry.path()).ok();
        }
    }
    Ok(())
}

mod tests {
    use super::super::{Journal, Outcome};

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

    #[tokio::test(flavor = "multi_thread")]
    async fn a_journal_copied_from_the_template_matches_a_freshly_bootstrapped_one() -> TestResult {
        let directory = tempfile::tempdir()?;
        let fresh = directory.path().join("fresh.db");
        drop(Journal::open_bootstrapped(&fresh).await?);
        let reopened = Journal::open_bootstrapped(&fresh).await?;
        let fresh_connection = reopened.connection().await?;
        let expected = schema(&reopened).await?;
        assert_eq!(expected.0, Journal::current_schema_version_for_test());
        assert!(
            expected.1.len() > 100,
            "the schema holds hundreds of objects"
        );
        let mut identities = vec![crate::journal_schema::instance_id(&fresh_connection).await?];
        for name in ["first.db", "second.db"] {
            let journal = Journal::open(&directory.path().join(name)).await?;
            assert_eq!(schema(&journal).await?, expected, "{name}");
            let id = journal.begin("template", name).await?;
            journal.finish(id, Outcome::Done).await?;
            identities.push(crate::journal_schema::instance_id(&journal.connection().await?).await?);
        }
        identities.sort();
        identities.dedup();
        assert_eq!(identities.len(), 3, "every journal has its own identity");
        Ok(())
    }
}
