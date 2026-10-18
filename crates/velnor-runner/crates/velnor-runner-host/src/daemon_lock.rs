//! Process locks and a durable Docker-engine journal lineage anchor.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::HostError;
use serde::{Deserialize, Serialize};

#[path = "daemon_lock_filesystem.rs"]
mod filesystem;
#[path = "daemon_lock_identity.rs"]
mod identity;

pub(crate) use filesystem::canonical_journal_path;
use filesystem::{ensure_regular_or_missing, open_lock, private_directory};
pub(super) use identity::{engine_id_valid, engine_key, instance_id_valid};

const ANCHOR_VERSION: u8 = 1;
const MAX_ANCHOR_BYTES: u64 = 16 * 1024;
const ANCHOR_DIR: &str = "runner-lineage";

/// Held advisory lock. Dropping the file releases it.
#[derive(Debug)]
pub struct DaemonLock {
    file: File,
    device: u64,
    inode: u64,
}

impl DaemonLock {
    /// Acquire the lock or fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] when the file cannot be created or is held.
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
            })
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            Err(HostError::Lock)
        }
    }

    /// The lock is held while the advisory file is still open.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.file.metadata().is_ok()
    }

    /// Check that the retained advisory lock still protects this exact path.
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

/// Cloneable engine-wide lock and monotonic journal-lineage anchor.
#[derive(Debug, Clone)]
pub(crate) struct EngineLineageGuard {
    inner: Arc<GuardInner>,
}

#[derive(Debug)]
struct GuardInner {
    _lock_file: File,
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
    /// Acquire the persistent engine-scoped lock under Velnor's state directory.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] if held or [`HostError::Path`] for unsafe state.
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

    #[cfg(test)]
    pub(crate) fn acquire_at_for_tests(engine_id: &str, root: &Path) -> Result<Self, HostError> {
        Self::acquire_at(engine_id, root)
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
                _lock_file: lock_file,
                engine_id: engine_id.to_owned(),
                anchor_path,
                state: Mutex::new(None),
            }),
        })
    }

    /// Bind this engine to one canonical journal path and database instance.
    ///
    /// One revision of database-ahead state recovers; all other mismatches fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for mixed lineage, rollback, or an
    /// invalid anchor, and [`HostError::Path`] for an unsafe journal path.
    pub(crate) fn verify_lineage(
        &self,
        canonical_path: &Path,
        instance_id: &str,
        revision: u64,
    ) -> Result<(), HostError> {
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
        let partial_write = has_partial_anchor(&self.inner.anchor_path)?;
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
                    if partial_write {
                        return Err(HostError::Journal);
                    }
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

    pub(crate) fn engine_id(&self) -> &str {
        &self.inner.engine_id
    }

    pub(crate) fn same_guard(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Advance the external anchor after the matching database commit.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an invalid revision, changed anchor, or I/O failure.
    pub(crate) fn advance_revision(&self, revision: u64) -> Result<(), HostError> {
        let mut state = self.inner.state.lock().map_err(|_| HostError::Journal)?;
        let current = state.as_ref().ok_or(HostError::Journal)?;
        if current.revision == revision {
            return if read_anchor(&self.inner.anchor_path)?.as_ref() == Some(current) {
                Ok(())
            } else {
                Err(HostError::Journal)
            };
        }
        if current.revision.checked_add(1) != Some(revision)
            || read_anchor(&self.inner.anchor_path)?.as_ref() != Some(current)
        {
            return Err(HostError::Journal);
        }
        let mut next = current.clone();
        next.revision = revision;
        write_anchor(&self.inner.anchor_path, &next)?;
        *state = Some(next);
        Ok(())
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

fn read_anchor(path: &Path) -> Result<Option<Anchor>, HostError> {
    ensure_regular_or_missing(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(HostError::Journal),
    };
    if metadata.len() > MAX_ANCHOR_BYTES {
        return Err(HostError::Journal);
    }
    let bytes = fs::read(path).map_err(|_| HostError::Journal)?;
    let anchor = serde_json::from_slice(&bytes).map_err(|_| HostError::Journal)?;
    Ok(Some(anchor))
}

fn has_partial_anchor(path: &Path) -> Result<bool, HostError> {
    let parent = path.parent().ok_or(HostError::Path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(HostError::Path)?;
    let prefix = format!(".{name}.tmp-");
    for entry in fs::read_dir(parent).map_err(|_| HostError::Journal)? {
        let entry = entry.map_err(|_| HostError::Journal)?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|candidate| candidate.starts_with(&prefix))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_anchor(path: &Path, anchor: &Anchor) -> Result<(), HostError> {
    ensure_regular_or_missing(path)?;
    let bytes = serde_json::to_vec(anchor).map_err(|_| HostError::Journal)?;
    let parent = path.parent().ok_or(HostError::Path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(HostError::Path)?;
    let temporary = parent.join(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| HostError::Journal)?;
    file.write_all(&bytes).map_err(|_| HostError::Journal)?;
    file.sync_all().map_err(|_| HostError::Journal)?;
    drop(file);
    fs::rename(&temporary, path).map_err(|_| HostError::Journal)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Journal)
}

#[cfg(test)]
#[path = "daemon_lock_tests.rs"]
mod tests;
