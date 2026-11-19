// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::HostError;
use serde::{Deserialize, Serialize};

#[path = "daemon_lock_anchor.rs"]
mod anchor_io;
#[path = "daemon_lock/controller_run.rs"]
mod controller_run;
#[path = "daemon_lock_filesystem.rs"]
mod filesystem;
#[path = "daemon_lock_identity.rs"]
mod identity;

#[cfg(test)]
use anchor_io::write_anchor_with;
use anchor_io::{read_anchor, remove_stale_anchor_temporaries, sync_parent, write_anchor};
pub(crate) use filesystem::canonical_journal_path;
use filesystem::{open_lock, private_directory};
pub(super) use identity::{engine_id_valid, engine_key, instance_id_valid};

#[cfg(test)]
#[path = "daemon_lock/anchor_temporary_tests.rs"]
mod anchor_temporary_tests;
#[cfg(test)]
#[path = "daemon_lock/tests.rs"]
mod tests;

const ANCHOR_VERSION: u8 = 1;
const MAX_ANCHOR_BYTES: u64 = 16 * 1024;
const ANCHOR_DIR: &str = "runner-lineage";

#[derive(Debug)]
pub struct DaemonLock {
    file: File,
    device: u64,
    inode: u64,
    controller_running: AtomicBool,
}

impl DaemonLock {
    pub fn try_acquire(path: &Path) -> Result<Self, HostError> {
        let file = open_lock(path)?;
        file.try_lock().map_err(|_| HostError::Lock)?;
        let metadata = file.metadata().map_err(|_| HostError::Lock)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                file,
                device: metadata.dev(),
                inode: metadata.ino(),
                controller_running: AtomicBool::new(false),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            Err(HostError::Lock)
        }
    }

    #[must_use]
    pub fn is_held(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.file
                .metadata()
                .is_ok_and(|metadata| metadata.nlink() > 0)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    #[must_use]
    pub(crate) fn matches_path(&self, path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let Ok(canonical) = fs::canonicalize(path) else {
                return false;
            };
            if canonical != path {
                return false;
            }
            let Ok(path_metadata) = fs::symlink_metadata(path) else {
                return false;
            };
            if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
                return false;
            }
            let Ok(descriptor_metadata) = self.file.metadata() else {
                return false;
            };
            path_metadata.dev() == self.device
                && path_metadata.ino() == self.inode
                && descriptor_metadata.dev() == self.device
                && descriptor_metadata.ino() == self.inode
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            false
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EngineLineageGuard {
    inner: Arc<GuardInner>,
}

#[derive(Debug)]
struct GuardInner {
    lock_file: File,
    lock_path: PathBuf,
    engine_id: String,
    anchor_path: PathBuf,
    state: Mutex<Option<Anchor>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Anchor {
    version: u8,
    engine_id: String,
    journal_path: String,
    instance_id: String,
    revision: u64,
}

impl EngineLineageGuard {
    pub(crate) fn acquire(engine_id: &str) -> Result<Self, HostError> {
        if !engine_id_valid(engine_id) {
            return Err(HostError::Docker);
        }
        let home = std::env::var_os("HOME").ok_or(HostError::Path)?;
        let home = fs::canonicalize(home).map_err(|_| HostError::Path)?;
        let root = home
            .join("Library/Application Support/Velnor")
            .join(ANCHOR_DIR);
        Self::acquire_process_at(engine_id, &root)
    }

    fn acquire_process_at(engine_id: &str, root: &Path) -> Result<Self, HostError> {
        let mut guards = process_guards().lock().map_err(|_| HostError::Lock)?;
        if let Some(guard) = guards.get(engine_id) {
            return Ok(guard.clone());
        }
        let guard = Self::acquire_at(engine_id, root)?;
        guards.insert(engine_id.to_owned(), guard.clone());
        Ok(guard)
    }

    fn acquire_at(engine_id: &str, root: &Path) -> Result<Self, HostError> {
        if !engine_id_valid(engine_id) {
            return Err(HostError::Docker);
        }
        let root = private_directory(root)?;
        let key = engine_key(engine_id);
        let lock_path = root.join(format!("engine-{key}.lock"));
        let anchor_path = root.join(format!("engine-{key}.anchor"));
        let lock_file = open_lock(&lock_path)?;
        lock_file.try_lock().map_err(|_| HostError::Lock)?;
        Ok(Self {
            inner: Arc::new(GuardInner {
                lock_file,
                lock_path,
                engine_id: engine_id.to_owned(),
                anchor_path,
                state: Mutex::new(None),
            }),
        })
    }

    pub(crate) fn verify_lineage(
        &self,
        canonical_path: &Path,
        instance_id: &str,
        revision: u64,
    ) -> Result<(), HostError> {
        self.ensure_lock_current()?;
        let path = canonical_journal_path(canonical_path)?;
        let path_text = path.to_str().ok_or(HostError::Path)?.to_owned();
        if !instance_id_valid(instance_id) {
            return Err(HostError::Journal);
        }
        let desired = Anchor {
            version: ANCHOR_VERSION,
            engine_id: self.inner.engine_id.clone(),
            journal_path: path_text,
            instance_id: instance_id.to_owned(),
            revision,
        };
        let mut state = self.inner.state.lock().map_err(|_| HostError::Journal)?;
        let current = state.clone();
        let existing = match current {
            Some(current) => {
                check_lineage(&current, &desired)?;
                if read_anchor(&self.inner.anchor_path)?.as_ref() != Some(&current) {
                    return Err(HostError::Journal);
                }
                current
            }
            None => {
                if let Some(anchor) = read_anchor(&self.inner.anchor_path)? {
                    anchor
                } else {
                    remove_stale_anchor_temporaries(&self.inner.anchor_path)?;
                    write_anchor(&self.inner.anchor_path, &desired)?;
                    *state = Some(desired);
                    return Ok(());
                }
            }
        };
        check_lineage(&existing, &desired)?;
        let recovered = recover_revision(existing, desired)?;
        if read_anchor(&self.inner.anchor_path)?.as_ref() != Some(&recovered) {
            write_anchor(&self.inner.anchor_path, &recovered)?;
        }
        *state = Some(recovered);
        Ok(())
    }

    fn ensure_lock_current(&self) -> Result<(), HostError> {
        if lock_file_is_linked_at(&self.inner.lock_file, &self.inner.lock_path) {
            Ok(())
        } else {
            Err(HostError::Lock)
        }
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.inner.engine_id
    }

    pub(crate) fn same_guard(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn advance_revision(&self, revision: u64) -> Result<(), HostError> {
        self.ensure_lock_current()?;
        let mut state = self.inner.state.lock().map_err(|_| HostError::Journal)?;
        let current = state.as_ref().ok_or(HostError::Journal)?;
        if current.revision == revision {
            return if read_anchor(&self.inner.anchor_path)?.as_ref() == Some(current) {
                Ok(())
            } else {
                Err(HostError::Journal)
            };
        }
        if current.revision.checked_add(1) != Some(revision) {
            return Err(HostError::Journal);
        }
        let mut next = current.clone();
        next.revision = revision;
        let on_disk = read_anchor(&self.inner.anchor_path)?;
        if on_disk.as_ref() == Some(&next) {
            // A previous advance renamed this exact record into place but
            // failed to sync the directory. Retry the sync instead of
            // rejecting a revision that is already published.
            sync_parent(&self.inner.anchor_path)?;
        } else if on_disk.as_ref() == Some(current) {
            write_anchor(&self.inner.anchor_path, &next)?;
        } else {
            return Err(HostError::Journal);
        }
        *state = Some(next);
        Ok(())
    }
}

fn lock_file_is_linked_at(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let Ok(held) = file.metadata() else {
            return false;
        };
        let Ok(linked) = fs::symlink_metadata(path) else {
            return false;
        };
        !linked.file_type().is_symlink()
            && linked.is_file()
            && held.nlink() > 0
            && linked.dev() == held.dev()
            && linked.ino() == held.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path);
        false
    }
}

fn process_guards() -> &'static Mutex<HashMap<String, EngineLineageGuard>> {
    static GUARDS: OnceLock<Mutex<HashMap<String, EngineLineageGuard>>> = OnceLock::new();
    GUARDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn check_lineage(existing: &Anchor, desired: &Anchor) -> Result<(), HostError> {
    if existing.version != ANCHOR_VERSION
        || existing.engine_id != desired.engine_id
        || existing.journal_path != desired.journal_path
        || existing.instance_id != desired.instance_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn recover_revision(existing: Anchor, desired: Anchor) -> Result<Anchor, HostError> {
    if existing.revision == desired.revision {
        return Ok(existing);
    }
    if existing.revision.checked_add(1) == Some(desired.revision) {
        return Ok(desired);
    }
    Err(HostError::Journal)
}
