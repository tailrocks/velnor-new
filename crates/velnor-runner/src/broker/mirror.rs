// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/git_mirror.rs

use std::ffi::OsString;
use std::fs::TryLockError;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags};
use rustix::io::Errno;

use super::facts::TrustedFacts;
use super::git::{fetch_env, git, git_args, has_commit, has_tree};
use super::location::mirror_path;
use super::proto::Window;
use super::repair::{Scan, drop_broken_refs, holds_commit, quarantine_corrupt};
use super::transfer::{Transfer, fetch_wanted};

async fn lock(path: &Path) -> Result<std::fs::File> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .custom_flags(
            (OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC)
                .bits()
                .cast_signed(),
        )
        .open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;
    let meta = file
        .metadata()
        .await
        .with_context(|| format!("inspect {}", path.display()))?;
    if !meta.is_file() {
        bail!("mirror lock {} is not a regular file", path.display());
    }
    let file = file.into_std().await;
    let mut backoff = Duration::from_millis(5);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_millis(100));
            }
            Err(TryLockError::Error(error)) => {
                return Err(error).with_context(|| format!("lock {}", path.display()));
            }
        }
    }
}

const MIRROR_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tbare = true\n";
const MIRROR_HEAD: &str = "ref: refs/heads/main\n";

async fn remove_entry(path: &Path) -> Result<()> {
    let removed = match tokio::fs::symlink_metadata(path).await {
        Ok(meta) if meta.is_dir() => tokio::fs::remove_dir_all(path).await,
        Ok(_) => tokio::fs::remove_file(path).await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    };
    removed.with_context(|| format!("remove {}", path.display()))
}

async fn install(mirror: &Path, name: &str, body: &str) -> Result<()> {
    let target = mirror.join(name);
    let staged = mirror.join(format!("{name}.velnor"));
    remove_entry(&staged).await?;
    let write = {
        let staged = staged.clone();
        let body = body.to_owned();
        move || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staged)?;
            file.write_all(body.as_bytes())
        }
    };
    tokio::task::spawn_blocking(write)
        .await
        .context("wait for the mirror write")?
        .with_context(|| format!("write {}", staged.display()))?;
    tokio::fs::rename(&staged, &target)
        .await
        .with_context(|| format!("replace {}", target.display()))
}

fn valid_head(head: &str) -> bool {
    head.strip_prefix("ref: refs/heads/")
        .and_then(|name| name.strip_suffix('\n'))
        .is_some_and(|name| {
            !name.is_empty()
                && !name.contains("..")
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')
                })
        })
}

const MAX_HEAD_BYTES: u64 = 512;
const MAX_REF_DEPTH: usize = 32;

fn read_head(path: &Path) -> Option<String> {
    let flags = OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits().cast_signed())
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut head = String::new();
    file.take(MAX_HEAD_BYTES).read_to_string(&mut head).ok()?;
    Some(head)
}

async fn reset_mirror(mirror: &Path) -> Result<()> {
    remove_entry(&mirror.join("commondir")).await?;
    remove_entry(&mirror.join(".git")).await?;
    let head = {
        let path = mirror.join("HEAD");
        tokio::task::spawn_blocking(move || read_head(&path))
            .await
            .context("wait for the mirror HEAD read")?
    };
    if !head.is_some_and(|head| valid_head(&head)) {
        install(mirror, "HEAD", MIRROR_HEAD).await?;
    }
    install(mirror, "config", MIRROR_CONFIG).await
}

#[derive(PartialEq, Eq)]
enum Refs {
    Sound,
    Replace,
}

fn remove_locks_in(dir: &OwnedFd, depth: usize) -> Result<Refs> {
    let entries = Dir::read_from(dir).context("read a refs directory")?;
    for entry in entries {
        let entry = entry.context("read a refs entry")?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let kind = rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
            .map(|stat| FileType::from_raw_mode(stat.st_mode));
        match kind {
            Ok(FileType::Directory) => {
                if depth >= MAX_REF_DEPTH {
                    return Ok(Refs::Replace);
                }
                let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
                match rustix::fs::openat(dir, name, flags, Mode::empty()) {
                    Ok(child) => {
                        if remove_locks_in(&child, depth + 1)? == Refs::Replace {
                            return Ok(Refs::Replace);
                        }
                    }
                    Err(Errno::NOENT | Errno::LOOP | Errno::NOTDIR) => {}
                    Err(error) => return Err(error).context("open a refs directory"),
                }
            }
            Ok(FileType::RegularFile) if name.to_bytes().ends_with(b".lock") => {
                match rustix::fs::unlinkat(dir, name, AtFlags::empty()) {
                    Ok(()) | Err(Errno::NOENT) => {}
                    Err(error) => return Err(error).context("remove a ref lock"),
                }
            }
            Ok(_) | Err(Errno::NOENT) => {}
            Err(error) => return Err(error).context("inspect a refs entry"),
        }
    }
    Ok(Refs::Sound)
}

fn remove_ref_locks_blocking(mirror: &Path) -> Result<Refs> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let mirror_fd = rustix::fs::openat(CWD, mirror, flags, Mode::empty())
        .with_context(|| format!("open {}", mirror.display()))?;
    match rustix::fs::openat(&mirror_fd, "refs", flags | OFlags::NOFOLLOW, Mode::empty()) {
        Ok(refs) => remove_locks_in(&refs, 0),
        Err(Errno::NOENT) => Ok(Refs::Sound),
        Err(Errno::LOOP | Errno::NOTDIR) => Ok(Refs::Replace),
        Err(error) => Err(error).context("open the mirror refs directory"),
    }
}

async fn remove_ref_locks(mirror: &Path) -> Result<()> {
    let mirror = mirror.to_owned();
    let replace = {
        let mirror = mirror.clone();
        tokio::task::spawn_blocking(move || remove_ref_locks_blocking(&mirror))
            .await
            .context("wait for the ref lock cleanup")??
    };
    if replace == Refs::Replace {
        let refs = mirror.join("refs");
        remove_entry(&refs).await?;
        tokio::fs::create_dir(&refs)
            .await
            .with_context(|| format!("create {}", refs.display()))?;
    }
    Ok(())
}

async fn open(root: &Path, facts: &TrustedFacts) -> Result<(PathBuf, std::fs::File)> {
    let mirror = mirror_path(root, facts)?;
    let mut lock_file = mirror.clone().into_os_string();
    lock_file.push(".lock");
    if let Some(parent) = mirror.parent() {
        tokio::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .await
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let lock = lock(Path::new(&lock_file)).await?;
    prepare(&mirror).await?;
    Ok((mirror, lock))
}

async fn refetch(
    mirror: &Path,
    clone_url: &str,
    facts: &TrustedFacts,
    env: &[(OsString, OsString)],
) -> Result<()> {
    fetch_wanted(mirror, clone_url, facts, env, Transfer::Refetch)
        .await
        .map_err(|error| {
            if format!("{error:#}").contains("unknown option") {
                error.context(
                    "repair the mirror with git fetch --refetch, which needs git 2.36 or newer",
                )
            } else {
                error
            }
        })
}

fn refused_by_the_server(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("upload-pack: not our ref ")
}

pub(crate) async fn fetch(
    root: &Path,
    facts: &TrustedFacts,
    token: Option<&str>,
) -> Result<Vec<String>> {
    let (mirror, _lock) = open(root, facts).await?;
    let sha = facts.sha.as_str();
    remove_ref_locks(&mirror).await?;
    let notices = drop_broken_refs(&mirror).await?;
    if has_tree(&mirror, sha).await {
        return Ok(notices);
    }
    let clone_url = facts.clone_url();
    let env = fetch_env(facts.server_url.url(), token);
    let mut fetched = fetch_wanted(&mirror, &clone_url, facts, &env, Transfer::Incremental).await;
    if !has_tree(&mirror, sha).await {
        let scan = quarantine_corrupt(&mirror, sha, Window::COMMIT).await?;
        if scan == Scan::Quarantined
            || !notices.is_empty()
            || holds_commit(&mirror, sha).await
            || fetched
                .as_ref()
                .is_err_and(|error| !refused_by_the_server(error))
        {
            fetched = refetch(&mirror, &clone_url, facts, &env).await;
        }
    }
    if has_tree(&mirror, sha).await {
        return Ok(notices);
    }
    fetched?;
    if has_commit(&mirror, sha).await {
        bail!("checkout: commit {sha} from {clone_url} arrived without all of its objects");
    }
    bail!("checkout: commit {sha} not found in {clone_url}")
}

pub(crate) enum Repair {
    Sound,
    Repaired(Vec<String>),
}

pub(crate) async fn repair(
    root: &Path,
    facts: &TrustedFacts,
    token: Option<&str>,
    window: Window,
) -> Result<Repair> {
    let (mirror, _lock) = open(root, facts).await?;
    if quarantine_corrupt(&mirror, facts.sha.as_str(), window).await? == Scan::Sound {
        return Ok(Repair::Sound);
    }
    remove_ref_locks(&mirror).await?;
    let notices = drop_broken_refs(&mirror).await?;
    let env = fetch_env(facts.server_url.url(), token);
    refetch(&mirror, &facts.clone_url(), facts, &env).await?;
    Ok(Repair::Repaired(notices))
}

async fn is_dir(path: &Path) -> bool {
    tokio::fs::symlink_metadata(path)
        .await
        .is_ok_and(|meta| meta.is_dir())
}

async fn is_file(path: &Path) -> bool {
    tokio::fs::symlink_metadata(path)
        .await
        .is_ok_and(|meta| meta.is_file())
}

async fn prepare(mirror: &Path) -> Result<()> {
    let config = mirror.join("config");
    let objects = mirror.join("objects");
    let refs = mirror.join("refs");
    let mut broken = Vec::new();
    if !is_file(&config).await {
        broken.push(&config);
    }
    for dir in [&objects, &refs] {
        if !is_dir(dir).await {
            broken.push(dir);
        }
    }
    if !broken.is_empty() {
        for path in broken {
            remove_entry(path).await?;
        }
        let mirror_arg = mirror
            .to_str()
            .with_context(|| format!("path {} is not UTF-8", mirror.display()))?;
        git(&git_args(None, &["init", "-q", "--bare", mirror_arg]), &[]).await?;
    }
    reset_mirror(mirror).await
}
