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
    cache_root_path: PathBuf,
    cache_root_identity: Option<CacheRootIdentity>,
    canonical_cache_root: Option<PathBuf>,
    resolved_directories: BTreeMap<PathBuf, CachedDirectory>,
}

#[derive(PartialEq, Eq)]
enum CacheRootIdentity {
    Missing,
    Present(PathIdentity),
}

#[derive(PartialEq, Eq)]
struct PathIdentity {
    is_symlink: bool,
    file_identity: Option<FileIdentity>,
    symlink_target: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

struct CachedDirectory {
    resolved: PathBuf,
    identity: PathIdentity,
}

impl PathIdentity {
    fn read(path: &Path, metadata: &fs::Metadata) -> std::io::Result<Self> {
        let is_symlink = metadata.file_type().is_symlink();
        Ok(Self {
            is_symlink,
            file_identity: file_identity(metadata),
            symlink_target: is_symlink.then(|| fs::read_link(path)).transpose()?,
        })
    }
}

impl CacheRootIdentity {
    fn read(path: &Path) -> std::io::Result<Self> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Self::Present(PathIdentity::read(path, &metadata)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(error) => Err(error),
        }
    }
}

impl CacheAdmission {
    /// Resolve the configured cache root once for this index operation.
    pub(crate) fn new(repository_root: &Path) -> Self {
        let cache_root_path = repository_root.join(RESERVED_CACHE_PATH);
        let initial_identity = CacheRootIdentity::read(&cache_root_path).ok();
        let canonical_cache_root = cache_root_path.canonicalize().ok();
        let current_identity = CacheRootIdentity::read(&cache_root_path).ok();
        let cache_root_identity = match (initial_identity, current_identity) {
            (Some(initial), Some(current)) if initial == current => Some(current),
            _ => None,
        };
        Self {
            cache_root_path,
            cache_root_identity,
            canonical_cache_root,
            resolved_directories: BTreeMap::new(),
        }
    }

    /// Reject a cache root whose link or resolved target changed after setup.
    pub(crate) fn ensure_cache_root_unchanged(&self) -> Result<(), IndexError> {
        let expected = self.cache_root_identity.as_ref().ok_or_else(|| {
            IndexError::ReadFailed(format!(
                "reserved cache root identity unavailable: {}",
                self.cache_root_path.display()
            ))
        })?;
        let current = CacheRootIdentity::read(&self.cache_root_path)
            .map_err(|error| IndexError::ReadFailed(error.to_string()))?;
        if &current != expected
            || self.cache_root_path.canonicalize().ok().as_ref()
                != self.canonical_cache_root.as_ref()
        {
            return Err(self.cache_root_changed());
        }
        Ok(())
    }

    fn cache_root_changed(&self) -> IndexError {
        IndexError::ReadFailed(format!(
            "reserved cache root changed during index operation: {}",
            self.cache_root_path.display()
        ))
    }

    /// Whether a resolved target lies inside the configured cache root.
    pub(crate) fn target_is_reserved(&self, target: &Path) -> bool {
        self.canonical_cache_root
            .as_deref()
            .is_some_and(|cache_root| target.starts_with(cache_root))
    }

    /// Whether an enumerated path resolves through any symlink into the cache.
    ///
    /// Ordinary directories are memoized so Git-backed indexing does not
    /// repeatedly inspect the same parent path for every listed file.
    pub(crate) fn listed_path_is_reserved(
        &mut self,
        repository_root: &Path,
        relative: &str,
    ) -> Result<bool, IndexError> {
        self.ensure_cache_root_unchanged()?;
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
        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            let final_component = index + 1 == components.len();
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(IndexError::ReadFailed(error.to_string())),
            };
            if let Some(cached) = self.resolved_directories.get(&current)
                && cached.is_current(&current, &resolved, component.as_os_str(), &metadata)?
            {
                resolved.clone_from(&cached.resolved);
                if self.target_is_reserved(&resolved) {
                    return Ok(true);
                }
                continue;
            }
            if metadata.file_type().is_symlink() {
                resolved = current.canonicalize().map_err(|error| {
                    IndexError::SymlinkLoop(format!("{}: {error}", current.display()))
                })?;
            } else {
                resolved.push(component.as_os_str());
            }
            if self.target_is_reserved(&resolved) {
                return Ok(true);
            }
            if !final_component {
                let identity = PathIdentity::read(&current, &metadata)
                    .map_err(|error| IndexError::ReadFailed(error.to_string()))?;
                if identity.file_identity.is_some() {
                    self.resolved_directories.insert(
                        current.clone(),
                        CachedDirectory {
                            resolved: resolved.clone(),
                            identity,
                        },
                    );
                }
            }
        }
        Ok(false)
    }
}

impl CachedDirectory {
    fn is_current(
        &self,
        path: &Path,
        resolved_parent: &Path,
        component: &std::ffi::OsStr,
        metadata: &fs::Metadata,
    ) -> Result<bool, IndexError> {
        let identity = PathIdentity::read(path, metadata)
            .map_err(|error| IndexError::ReadFailed(error.to_string()))?;
        if identity != self.identity {
            return Ok(false);
        }
        if identity.is_symlink {
            let canonical = path
                .canonicalize()
                .map_err(|error| IndexError::SymlinkLoop(format!("{}: {error}", path.display())))?;
            return Ok(canonical == self.resolved);
        }
        Ok(resolved_parent.join(component) == self.resolved)
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
