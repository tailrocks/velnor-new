//! Staged tree writing: regular files and symbolic links.

use std::io::Write;
use std::path::Path;

use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;

/// Write every rendered file and symbolic link under `github_dir`.
///
/// Parents are created and verified against symlinks before use; leaves are
/// created exclusively, so overwrites and swapped links refuse instead of
/// diverting writes.
pub(crate) fn write_tree(github_dir: &Path, tree: &RenderedTree) -> Result<(), OrchestratorError> {
    for file in &tree.files {
        let rel = file.path.strip_prefix(".github/").unwrap_or(&file.path);
        let dest = github_dir.join(rel);
        ensure_parent_dirs_safe(github_dir, &dest)?;
        if std::fs::symlink_metadata(&dest).is_ok_and(|meta| meta.is_symlink()) {
            return Err(OrchestratorError::UnsafePath {
                path: dest.display().to_string(),
                reason: "symlink_refused".to_owned(),
            });
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    OrchestratorError::OverwriteRefused {
                        path: dest.display().to_string(),
                    }
                } else {
                    OrchestratorError::io(dest.display().to_string(), err.to_string())
                }
            })?
            .write_all(file.bytes.as_bytes())
            .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))?;
    }
    for link in &tree.symlinks {
        let rel = link.path.strip_prefix(".github/").unwrap_or(&link.path);
        let dest = github_dir.join(rel);
        ensure_parent_dirs_safe(github_dir, &dest)?;
        if std::fs::symlink_metadata(&dest).is_ok() {
            return Err(OrchestratorError::OverwriteRefused {
                path: dest.display().to_string(),
            });
        }
        create_symlink(&link.target, &dest)?;
    }
    Ok(())
}

/// Verify ancestor directory chain contains no symlinks.
fn ensure_parent_dirs_safe(github_dir: &Path, dest: &Path) -> Result<(), OrchestratorError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| OrchestratorError::io(parent.display().to_string(), err.to_string()))?;
        let mut current = Some(parent);
        while let Some(level) = current {
            if !level.starts_with(github_dir) {
                break;
            }
            if std::fs::symlink_metadata(level).is_ok_and(|meta| meta.is_symlink()) {
                return Err(OrchestratorError::UnsafePath {
                    path: level.display().to_string(),
                    reason: "symlink_refused".to_owned(),
                });
            }
            current = if level == github_dir {
                None
            } else {
                level.parent()
            };
        }
    }
    Ok(())
}

/// Create a symbolic link from `target` to `dest`.
fn create_symlink(target: &str, dest: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, dest)
            .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, dest).map_err(|err| {
            OrchestratorError::io(
                dest.display().to_string(),
                format!("symlink_failed:{err} (Windows requires Developer Mode or SeCreateSymbolicLinkPrivilege)"),
            )
        })
    }
}
