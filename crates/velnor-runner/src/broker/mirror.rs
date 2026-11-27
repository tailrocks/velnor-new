// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/git_mirror.rs

use std::future::Future;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use rustix::fs::OFlags;

use super::facts::TrustedFacts;
use super::git::{HELD_LOCK, fetch_env, git, git_args, has_commit, has_tree};
use super::location::mirror_path;
use super::proto::{FailedStep, Window};
use super::repair::{Scan, drop_broken_refs, holds_commit, quarantine_corrupt};
use super::transfer::{Transfer, fetch_wanted};
use super::within;

mod dirty;
mod held;
mod owned;
mod pin;
mod refetch;
mod refs;
mod sound;

use held::Held;
use owned::Sweep;
pub(crate) use pin::verify_pin;
use refetch::{Refetch, refetch};

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
    if is_dir(&target).await {
        remove_entry(&target).await?;
    }
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

async fn locked<T, Work>(
    root: &Path,
    facts: &TrustedFacts,
    cap: Duration,
    what: &str,
    work: impl FnOnce(PathBuf, Held) -> Work,
) -> Result<T>
where
    Work: Future<Output = Result<T>>,
{
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
    let held = Held::acquire(Path::new(&lock_file)).await?;
    HELD_LOCK
        .scope(held.file(), async {
            prepare(&mirror, &held, SystemTime::now()).await?;
            let done = within(what, cap, work(mirror.clone(), held.clone())).await;
            if done.is_ok() {
                dirty::clear(&mirror).await;
            }
            done
        })
        .await
}

fn refused_by_the_server(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("upload-pack: not our ref ")
}

pub(crate) async fn fetch(
    root: &Path,
    facts: &TrustedFacts,
    token: Option<&str>,
    cap: Duration,
) -> Result<Vec<String>> {
    let what = "fetching into the mirror";
    locked(root, facts, cap, what, |mirror, held| {
        fetch_locked(mirror, held, facts, token)
    })
    .await
}

async fn fetch_locked(
    mirror: PathBuf,
    held: Held,
    facts: &TrustedFacts,
    token: Option<&str>,
) -> Result<Vec<String>> {
    let mirror = mirror.as_path();
    let sha = facts.sha.as_str();
    refs::tidy(mirror, &held).await?;
    let notices = drop_broken_refs(mirror).await?;
    if has_tree(mirror, sha).await {
        return Ok(notices);
    }
    let clone_url = facts.clone_url();
    let env = fetch_env(facts.server_url.url(), token);
    let mut fetched = fetch_wanted(mirror, &clone_url, facts, &env, Transfer::Incremental).await;
    if !has_tree(mirror, sha).await {
        let scan = quarantine_corrupt(mirror, sha, Window::COMMIT).await?;
        if scan == Scan::Quarantined
            || !notices.is_empty()
            || holds_commit(mirror, sha).await
            || fetched
                .as_ref()
                .is_err_and(|error| !refused_by_the_server(error))
        {
            fetched = refetch(mirror, &clone_url, Refetch::Job(facts), &env).await;
        }
    }
    if has_tree(mirror, sha).await {
        return Ok(notices);
    }
    fetched?;
    Err(missing_tree(mirror, sha, &clone_url).await)
}

async fn missing_tree(git_dir: &Path, sha: &str, clone_url: &str) -> anyhow::Error {
    if has_commit(git_dir, sha).await {
        anyhow!("checkout: commit {sha} from {clone_url} arrived without all of its objects")
    } else {
        anyhow!("checkout: commit {sha} not found in {clone_url}")
    }
}

pub(crate) enum Repair {
    Sound,
    Repaired(Vec<String>),
}

async fn repair_locked(
    mirror: &Path,
    held: &Held,
    facts: &TrustedFacts,
    token: Option<&str>,
    window: Window,
    failed: FailedStep,
) -> Result<Repair> {
    let sha = facts.sha.as_str();
    let remembers = failed.reads_only_the_commit();
    let cached = remembers && sound::is_fresh(mirror, sha, window).await;
    let scanned = if cached { Window::COMMIT } else { window };
    if quarantine_corrupt(mirror, sha, scanned).await? == Scan::Sound {
        if remembers && !cached {
            let _ = sound::record(mirror, sha, window).await;
        }
        return Ok(Repair::Sound);
    }
    refs::tidy(mirror, held).await?;
    let notices = drop_broken_refs(mirror).await?;
    let env = fetch_env(facts.server_url.url(), token);
    refetch(mirror, &facts.clone_url(), Refetch::Job(facts), &env).await?;
    Ok(Repair::Repaired(notices))
}

pub(crate) async fn repair(
    root: &Path,
    facts: &TrustedFacts,
    token: Option<&str>,
    window: Window,
    failed: FailedStep,
    cap: Duration,
) -> Result<Repair> {
    let what = "repairing the mirror";
    locked(root, facts, cap, what, |mirror, held| async move {
        repair_locked(&mirror, &held, facts, token, window, failed).await
    })
    .await
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

async fn prepare(mirror: &Path, held: &Held, acquired: SystemTime) -> Result<()> {
    let sweep = if dirty::pending(mirror).await {
        Sweep::Everything
    } else {
        Sweep::Packs
    };
    dirty::set(mirror).await?;
    owned::keep_only_owned(mirror, held).await?;
    let present = is_file(&mirror.join("config")).await
        && is_dir(&mirror.join("objects")).await
        && is_dir(&mirror.join("refs")).await;
    if !present {
        let mirror_arg = mirror
            .to_str()
            .with_context(|| format!("path {} is not UTF-8", mirror.display()))?;
        let args = git_args(None, &["init", "-q", "--bare", "--template=", mirror_arg]);
        git(&args, &[]).await?;
    }
    owned::sweep_objects(mirror, held, acquired, sweep).await?;
    reset_mirror(mirror).await
}
