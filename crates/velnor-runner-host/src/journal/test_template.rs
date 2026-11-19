// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[path = "test_template/prune.rs"]
mod prune;

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
    prune::prune_except(root, stamp)
}

mod tests {
    use super::super::{IntentState, Journal, Outcome};
    use super::prune::prune_except;
    use super::{Seed, install, publish_into, seed_from};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

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

    #[test]
    fn pruning_removes_stale_stamps_deeply_and_never_follows_a_link() -> TestResult {
        let directory = tempfile::tempdir()?;
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(&outside)?;
        std::fs::write(outside.join("precious"), b"x")?;
        let root = directory.path().join("root");
        std::fs::create_dir_all(root.join("keep").join("key"))?;
        std::fs::create_dir_all(root.join("old").join("key").join("deep"))?;
        std::fs::write(root.join("old").join("key").join("deep").join("db"), b"x")?;
        std::os::unix::fs::symlink(&outside, root.join("link"))?;
        prune_except(&root, "keep")?;
        assert!(root.join("keep").join("key").is_dir());
        assert!(!root.join("old").exists());
        assert!(std::fs::symlink_metadata(root.join("link")).is_err());
        assert!(outside.join("precious").exists());
        Ok(())
    }

    #[test]
    fn pruning_through_a_root_swapped_for_a_link_is_refused() -> TestResult {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target");
        std::fs::create_dir_all(target.join("victim"))?;
        let root = directory.path().join("root");
        std::os::unix::fs::symlink(&target, &root)?;
        assert!(prune_except(&root, "keep").is_err());
        assert!(target.join("victim").exists());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_journal_copied_from_the_template_behaves_like_a_freshly_bootstrapped_one()
    -> TestResult {
        let directory = tempfile::tempdir()?;
        let fresh = directory.path().join("fresh.db");
        let reference = Journal::open_bootstrapped(&fresh).await?;
        let mut identities = vec![reference.instance_id().await?];
        for name in ["first.db", "second.db"] {
            let path = directory.path().join(name);
            let journal = Journal::open(&path).await?;
            let before = journal.revision().await?;
            let id = journal.begin("template", name).await?;
            assert_eq!(journal.read(id).await?, IntentState::Pending);
            journal.finish(id, Outcome::Done).await?;
            assert_eq!(journal.read(id).await?, IntentState::Done);
            assert!(journal.revision().await? > before, "{name}");
            identities.push(journal.instance_id().await?);
            drop(journal);
            let reopened = Journal::open(&path).await?;
            assert_eq!(reopened.read(id).await?, IntentState::Done, "{name}");
        }
        identities.sort();
        identities.dedup();
        assert_eq!(identities.len(), 3, "every journal has its own identity");
        Ok(())
    }
}
