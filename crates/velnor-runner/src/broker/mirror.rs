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

pub(crate) fn mirror_path(root: &Path, facts: &TrustedFacts) -> Result<PathBuf> {
    let url = facts.server_url.url();
    let host = url
        .host_str()
        .map(|host| host.trim_matches(['[', ']']).replace(':', "+"))
        .filter(|host| !host.is_empty())
        .map_or_else(
            || "_local".to_owned(),
            |host| match url.port() {
                Some(port) => format!("{host}+{port}"),
                None => host,
            },
        );
    let valid = !host.starts_with('.')
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if !valid {
        bail!("mirror: server host '{host}' is not a valid path part");
    }
    let (owner, name) = facts.repository.owner_and_name();
    let mut path = root.join("git");
    for part in [host.as_str(), owner, name] {
        path.push(part.to_ascii_lowercase());
    }
    path.push(format!("{}.git", facts.repository_id));
    Ok(path)
}

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

pub(crate) async fn fetch(root: &Path, facts: &TrustedFacts, token: Option<&str>) -> Result<()> {
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
    let _lock = lock(Path::new(&lock_file)).await?;
    prepare(&mirror).await?;
    let sha = facts.sha.as_str();
    if has_tree(&mirror, sha).await {
        return Ok(());
    }
    if has_commit(&mirror, sha).await {
        remove_entry(&mirror).await?;
        prepare(&mirror).await?;
    }
    remove_ref_locks(&mirror).await?;
    let clone_url = facts.clone_url();
    let env = fetch_env(facts.server_url.url(), token);
    let mut fetched = fetch_refs(&mirror, &clone_url, facts.refspecs(), &env).await;
    if !has_tree(&mirror, sha).await
        && let Some(spec) = facts.sha_refspec()
    {
        fetched = fetch_refs(&mirror, &clone_url, vec![spec], &env).await;
    }
    if has_tree(&mirror, sha).await {
        return Ok(());
    }
    fetched?;
    bail!("checkout: commit {sha} not found in {clone_url}")
}

async fn prepare(mirror: &Path) -> Result<()> {
    let config = mirror.join("config");
    let config_is_file = tokio::fs::symlink_metadata(&config)
        .await
        .is_ok_and(|meta| meta.is_file());
    if !config_is_file {
        remove_entry(&config).await?;
        let mirror_arg = mirror
            .to_str()
            .with_context(|| format!("path {} is not UTF-8", mirror.display()))?;
        git(&git_args(None, &["init", "-q", "--bare", mirror_arg]), &[]).await?;
    }
    reset_mirror(mirror).await
}

async fn fetch_refs(
    mirror: &Path,
    clone_url: &str,
    specs: Vec<String>,
    env: &[(OsString, OsString)],
) -> Result<()> {
    let mut args = git_args(
        Some(mirror),
        &[
            "-c",
            "protocol.version=2",
            "-c",
            "gc.auto=0",
            "fetch",
            "--no-auto-maintenance",
            "--prune",
            "--no-tags",
            "--quiet",
            "--",
            clone_url,
        ],
    );
    args.extend(specs);
    git(&args, env).await
}
