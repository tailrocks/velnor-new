//! Shared admission for the repository-private discovery cache.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use super::index::IndexError;

const RESERVED_CACHE_PATH: &str = ".velnor/cache";

/// Whether a repository-relative path names the reserved cache root or a child.
pub(crate) fn is_reserved_cache_path(path: &str) -> bool {
    is_reserved_cache_path_bytes(path.as_bytes())
}

/// Whether raw Git path bytes name the reserved cache root or a child.
#[must_use]
pub fn is_reserved_cache_path_bytes(path: &[u8]) -> bool {
    let root = RESERVED_CACHE_PATH.as_bytes();
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with(b"/"))
}

/// Cache-root identity shared by listed and walked repository indexes.
pub(crate) struct CacheAdmission {
    canonical_cache_root: Option<PathBuf>,
    resolved_directories: BTreeMap<PathBuf, CachedDirectory>,
}

struct CachedDirectory {
    identity: FileIdentity,
    resolved: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl CacheAdmission {
    /// Resolve the configured cache root once for this index operation.
    pub(crate) fn new(repository_root: &Path) -> Self {
        Self {
            canonical_cache_root: repository_root
                .join(RESERVED_CACHE_PATH)
                .canonicalize()
                .ok(),
            resolved_directories: BTreeMap::new(),
        }
    }

    /// Whether a resolved target lies inside the configured cache root.
    pub(crate) fn target_is_reserved(&self, target: &Path) -> bool {
        self.canonical_cache_root
            .as_deref()
            .is_some_and(|cache_root| target.starts_with(cache_root))
    }

    /// Whether an enumerated path resolves through any symlink into the cache.
    ///
    /// Cached directory resolutions are reused only while every path prefix
    /// still has the same filesystem identity and no prefix is a symlink.
    pub(crate) fn listed_path_is_reserved(
        &mut self,
        repository_root: &Path,
        relative: &str,
    ) -> Result<bool, IndexError> {
        let normalized = relative
            .split('/')
            .filter(|component| !component.is_empty() && *component != ".")
            .collect::<Vec<_>>()
            .join("/");
        if is_reserved_cache_path(&normalized) {
            return Ok(true);
        }
        let components: Vec<_> = Path::new(relative).components().collect();
        let mut current = repository_root.to_path_buf();
        let mut resolved = repository_root.to_path_buf();
        let mut traversed_symlink = false;
        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            let final_component = index + 1 == components.len();
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(IndexError::ReadFailed(error.to_string())),
            };
            if metadata.file_type().is_symlink() {
                traversed_symlink = true;
                resolved = current.canonicalize().map_err(|error| {
                    IndexError::SymlinkLoop(format!("{}: {error}", current.display()))
                })?;
            } else {
                let identity = file_identity(&metadata);
                let cached = if traversed_symlink {
                    None
                } else {
                    identity.and_then(|identity| {
                        self.resolved_directories
                            .get(&current)
                            .filter(|cached| cached.identity == identity)
                    })
                };
                if let Some(cached) = cached {
                    resolved.clone_from(&cached.resolved);
                } else {
                    resolved.push(component.as_os_str());
                }
            }
            if self.target_is_reserved(&resolved) {
                return Ok(true);
            }
            if !final_component && !traversed_symlink {
                if let Some(identity) = file_identity(&metadata) {
                    self.resolved_directories.insert(
                        current.clone(),
                        CachedDirectory {
                            identity,
                            resolved: resolved.clone(),
                        },
                    );
                }
            }
        }
        Ok(false)
    }
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> Option<FileIdentity> {
    let inode = metadata.ino();
    (inode != 0).then_some(FileIdentity {
        device: metadata.dev(),
        inode,
    })
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> Option<FileIdentity> {
    None
}
