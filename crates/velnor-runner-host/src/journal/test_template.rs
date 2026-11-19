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
    match root() {
        Ok((root, stamp)) => seed_from(&root, &stamp, path, key),
        Err(_) => Seed::Missing,
    }
}

pub(super) fn seed_from(root: &Path, stamp: &str, path: &Path, key: &str) -> Seed {
    if path.exists() {
        return Seed::Existing;
    }
    let template = root.join(stamp).join(key);
    if !template.is_dir() {
        return Seed::Missing;
    }
    let Ok((parent, name)) = base(path) else {
        return Seed::Missing;
    };
    match install(&template, parent, name) {
        Ok(true) => Seed::Copied,
        Ok(false) => Seed::Existing,
        Err(_) => Seed::Missing,
    }
}

fn install(template: &Path, parent: &Path, name: &str) -> io::Result<bool> {
    let tag = std::process::id();
    let staged = |suffix: &str| parent.join(format!(".{name}.seed-{tag}{suffix}"));
    let result = (|| {
        for suffix in SUFFIXES {
            let source = template.join(format!("db{suffix}"));
            if source.exists() {
                fs::copy(&source, staged(suffix))?;
            }
        }
        for suffix in ["-wal", "-shm"] {
            if staged(suffix).exists() {
                fs::rename(staged(suffix), parent.join(format!("{name}{suffix}")))?;
            }
        }
        match fs::hard_link(staged(""), parent.join(name)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    })();
    for suffix in SUFFIXES {
        fs::remove_file(staged(suffix)).ok();
    }
    if result.is_err() {
        for suffix in ["-wal", "-shm"] {
            fs::remove_file(parent.join(format!("{name}{suffix}"))).ok();
        }
    }
    result
}

pub(super) fn publish(path: &Path, key: &str) {
    if let Ok((root, stamp)) = root() {
        publish_into(&root, &stamp, path, key).ok();
    }
}

pub(super) fn publish_into(root: &Path, stamp: &str, path: &Path, key: &str) -> io::Result<()> {
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(io::Error::other("journal template root is not a directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(root)?,
        Err(error) => return Err(error),
    }
    let (parent, name) = base(path)?;
    let stamp_dir = root.join(stamp);
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
    for entry in fs::read_dir(root)?.flatten() {
        if entry.file_name().to_str() != Some(stamp) {
            fs::remove_dir_all(entry.path()).ok();
        }
    }
    Ok(())
}

mod tests {
    use super::super::{Journal, Outcome};
    use super::{Seed, install, publish_into, seed_from};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    async fn schema(journal: &Journal) -> TestResult<(i64, Vec<String>)> {
        let connection = journal.connection().await?;
        let mut rows = connection.query("PRAGMA user_version", ()).await?;
        let version: i64 = rows.next().await?.ok_or("user_version")?.get(0)?;
        let mut rows = connection
            .query("SELECT name FROM sqlite_schema ORDER BY name", ())
            .await?;
        let mut objects = Vec::new();
        while let Some(row) = rows.next().await? {
            objects.push(row.get(0)?);
        }
        Ok((version, objects))
    }

    #[test]
    fn a_symlinked_template_root_is_refused_and_its_target_is_not_pruned() -> TestResult {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target");
        std::fs::create_dir_all(target.join("victim"))?;
        let root = directory.path().join("root");
        std::os::unix::fs::symlink(&target, &root)?;
        let db = directory.path().join("j.db");
        std::fs::write(&db, b"x")?;
        assert!(publish_into(&root, "stamp", &db, "key").is_err());
        assert!(target.join("victim").exists());
        assert!(!target.join("stamp").exists());
        Ok(())
    }

    #[test]
    fn an_install_that_loses_the_claim_reports_existing_and_leaves_no_staging() -> TestResult {
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("root");
        let source = directory.path().join("src.db");
        std::fs::write(&source, b"template")?;
        publish_into(&root, "stamp", &source, "key")?;
        let target = directory.path().join("dest.db");
        assert_eq!(seed_from(&root, "stamp", &target, "key"), Seed::Copied);
        assert_eq!(std::fs::read(&target)?, b"template");
        assert_eq!(seed_from(&root, "stamp", &target, "key"), Seed::Existing);
        let template = root.join("stamp").join("key");
        assert!(!install(&template, directory.path(), "dest.db")?);
        let leftovers = std::fs::read_dir(directory.path())?
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".seed-"))
            .count();
        assert_eq!(leftovers, 0);
        Ok(())
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
            identities
                .push(crate::journal_schema::instance_id(&journal.connection().await?).await?);
        }
        identities.sort();
        identities.dedup();
        assert_eq!(identities.len(), 3, "every journal has its own identity");
        Ok(())
    }
}
