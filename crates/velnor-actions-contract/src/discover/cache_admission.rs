//! Shared admission for the repository-private discovery cache.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    resolved_directories: BTreeMap<PathBuf, PathBuf>,
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
    /// Ordinary directories are memoized so Git-backed indexing does not
    /// repeatedly inspect the same parent path for every listed file.
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
        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            let final_component = index + 1 == components.len();
            if let Some(cached) = self.resolved_directories.get(&current) {
                resolved.clone_from(cached);
                continue;
            }
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(IndexError::ReadFailed(error.to_string())),
            };
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
                self.resolved_directories
                    .insert(current.clone(), resolved.clone());
            }
        }
        Ok(false)
    }
}
