// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::File;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{
    AtFlags, CWD, FlockOperation, Mode, OFlags, RawMode, fchmod, flock, fstat, mkdirat, openat,
    unlinkat,
};
use rustix::io::Errno;
use rustix::process::geteuid;
use velnor_cache_rules::DEFAULT_CACHE_BUDGET_BYTES;

use super::cache::{CacheOwner, DATA, MAX_DATA_BYTES, owner_parts, scope_hash, scope_parts};
use super::incoming::Upload;
use super::live::{LIVE, claim, reclaim};
use super::publish::{Target, sync_directory};
use super::safe_fs::{directory_flags, enter_directory_as, open_path, purge};

pub(super) const PRIVATE: RawMode = 0o700;
const SHARED: RawMode = 0o755;

pub(super) fn ensure_private(directory: &OwnedFd, what: &str) -> Result<()> {
    let stat = fstat(directory).context("inspect the directory")?;
    ensure!(
        stat.st_uid == geteuid().as_raw() && stat.st_mode & 0o7777 == PRIVATE,
        "{what} is not owned by this broker with mode {PRIVATE:o}"
    );
    Ok(())
}
const LOCK_POLL: Duration = Duration::from_millis(10);
const STORE_LOCK: &str = "store";

pub(super) type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn system_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[derive(Clone)]
pub(super) struct Layout {
    pub(super) root: PathBuf,
    id: String,
    next: Arc<AtomicU64>,
    live: Arc<Result<OwnedFd, String>>,
    pub(super) lock_wait: Duration,
    pub(super) max_data: u64,
    pub(super) budget: u64,
    pub(super) clock: Clock,
}

impl Layout {
    pub(super) fn new(root: PathBuf) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let live = claim(&root, &id).map_err(|error| format!("{error:#}"));
        if live.is_ok() {
            reclaim(&root, &id);
        }
        Self {
            root,
            id,
            next: Arc::new(AtomicU64::new(0)),
            live: Arc::new(live),
            lock_wait: Duration::from_secs(120),
            max_data: MAX_DATA_BYTES,
            budget: DEFAULT_CACHE_BUDGET_BYTES,
            clock: Arc::new(system_seconds),
        }
    }

    pub(super) fn now(&self) -> u64 {
        (self.clock)()
    }

    #[cfg(test)]
    pub(super) fn incoming(&self) -> PathBuf {
        self.root.join("incoming").join(&self.id)
    }

    pub(super) fn next_name(&self) -> String {
        self.next.fetch_add(1, Ordering::Relaxed).to_string()
    }

    pub(super) fn remove_all(&self) {
        let Ok(root) = open_path(&self.root) else {
            return;
        };
        for parent in ["incoming", "aside"] {
            if let Ok(dir) = openat(&root, parent, directory_flags(), Mode::empty()) {
                let _ = purge(dir.as_fd(), OsStr::new(&self.id));
            }
        }
        if let Ok(live) = openat(&root, LIVE, directory_flags(), Mode::empty()) {
            let _ = unlinkat(&live, &self.id, AtFlags::empty());
        }
    }

    fn require_live(&self) -> Result<()> {
        match &*self.live {
            Ok(_) => Ok(()),
            Err(error) => bail!("the broker could not claim its liveness lock: {error}"),
        }
    }

    pub(super) fn create_incoming(&self, name: &str) -> Result<(Arc<Upload>, File)> {
        self.require_live()?;
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("create {}", self.root.display()))?;
        let root = self.open_root()?;
        let (parent, _) = enter_directory_as(root.as_fd(), OsStr::new("incoming"), PRIVATE)
            .context("open the incoming directory")?;
        ensure_private(&parent, "the incoming directory")?;
        let (own, _) = enter_directory_as(parent.as_fd(), OsStr::new(&self.id), PRIVATE)
            .context("open the incoming directory of this broker")?;
        ensure_private(&own, "the incoming directory of this broker")?;
        mkdirat(own.as_fd(), name, Mode::from_bits_truncate(PRIVATE))
            .context("create the incoming upload directory")?;
        let dir = openat(own.as_fd(), name, directory_flags(), Mode::empty())
            .context("open the incoming upload directory")?;
        fchmod(&dir, Mode::from_bits_truncate(SHARED))
            .context("open the upload directory to the step uid")?;
        let data = openat(
            dir.as_fd(),
            DATA,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .with_context(|| format!("create {DATA} in the incoming upload"))?;
        Ok((Arc::new(Upload::new(own, dir, name)), File::from(data)))
    }

    pub(super) fn open_aside(&self) -> Result<OwnedFd> {
        self.require_live()?;
        let root = self.open_root()?;
        let (parent, _) = enter_directory_as(root.as_fd(), OsStr::new("aside"), PRIVATE)
            .context("open the aside directory")?;
        ensure_private(&parent, "the aside directory")?;
        let (own, _) = enter_directory_as(parent.as_fd(), OsStr::new(&self.id), PRIVATE)
            .context("open the aside directory of this broker")?;
        ensure_private(&own, "the aside directory of this broker")?;
        Ok(own)
    }

    pub(super) fn open_owner(&self, owner: &CacheOwner) -> Result<OwnedFd> {
        self.open_chain(&owner_parts(owner))
    }

    pub(super) fn open_scope(&self, target: &Target) -> Result<OwnedFd> {
        self.open_chain(&scope_parts(&target.owner, &target.scope))
    }

    fn open_chain(&self, parts: &[String]) -> Result<OwnedFd> {
        let mut directory = self.open_root()?;
        for part in parts {
            let (child, created) = enter_directory_as(directory.as_fd(), OsStr::new(part), SHARED)
                .with_context(|| format!("open the cache directory {part}"))?;
            if created {
                fchmod(&child, Mode::from_bits_truncate(SHARED))
                    .with_context(|| format!("open the cache directory {part} to the step uid"))?;
                sync_directory(&directory, "the parent of a new cache directory")?;
            }
            directory = child;
        }
        Ok(directory)
    }

    pub(super) fn lock(&self, target: &Target, abandoned: &AtomicBool) -> Result<OwnedFd> {
        self.lock_named(
            &scope_hash(&target.owner, &target.scope),
            "scope",
            abandoned,
        )
    }

    pub(super) fn lock_store(&self, abandoned: &AtomicBool) -> Result<OwnedFd> {
        self.lock_named(STORE_LOCK, "store", abandoned)
    }

    fn lock_named(&self, name: &str, what: &str, abandoned: &AtomicBool) -> Result<OwnedFd> {
        let root = self.open_root()?;
        drop(
            enter_directory_as(root.as_fd(), OsStr::new("locks"), PRIVATE)
                .context("open the lock directory")?
                .0,
        );
        let file = openat(
            CWD,
            self.root.join("locks").join(name),
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .with_context(|| format!("open the {what} lock"))?;
        let deadline = Instant::now() + self.lock_wait;
        loop {
            match flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(file),
                Err(Errno::WOULDBLOCK) => {}
                Err(error) => return Err(error).with_context(|| format!("lock the cache {what}")),
            }
            ensure!(
                !abandoned.load(Ordering::Relaxed),
                "the publish was abandoned while it waited for the cache {what} lock"
            );
            ensure!(
                Instant::now() < deadline,
                "timed out waiting for the cache {what} lock"
            );
            std::thread::sleep(LOCK_POLL);
        }
    }

    fn open_root(&self) -> Result<OwnedFd> {
        open_path(&self.root).with_context(|| format!("open {}", self.root.display()))
    }
}
