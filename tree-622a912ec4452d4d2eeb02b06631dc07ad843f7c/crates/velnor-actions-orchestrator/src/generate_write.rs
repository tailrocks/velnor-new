//! Staged tree writing plus the preserved-template read.
//!
//! Regular files and symbolic links go out; the repository-owned PR
//! template comes in for preservation (generated-file-contract §3).

use std::io::Write;
use std::path::Path;

use velnor_actions_contract::PULL_REQUEST_TEMPLATE_PATH;
use velnor_actions_workflow_renderer::render::{RenderedFile, RenderedTree};
use velnor_actions_workflow_renderer::tree::render_tree_with_preserved;

use crate::OrchestratorError;

/// Cap for preserved PR-template bytes.
const PRESERVED_TEMPLATE_MAX_BYTES: usize = 65_536;

/// Assemble the staged tree: generated files plus the preserved
/// repository-owned PR template, if the repository has one.
///
/// # Errors
///
/// Returns render errors for marker, token, path, or collision
/// failures, or preservation-gate failures.
pub(crate) fn assemble_tree(
    workflow: &str,
    actionlint: &str,
    extra: &[RenderedFile],
    root: &Path,
    version: &str,
) -> Result<RenderedTree, OrchestratorError> {
    let preserved = read_preserved_template(root)?;
    Ok(render_tree_with_preserved(
        workflow,
        actionlint,
        extra,
        preserved.as_ref(),
        version,
    )?)
}

/// Read the repository-owned PR template for preservation, if present.
///
/// Missing stays missing (historical behavior); anything that is not
/// a regular file, anything past the size cap, and non-UTF-8 bytes
/// fail closed before any write.
pub(crate) fn read_preserved_template(
    root: &Path,
) -> Result<Option<RenderedFile>, OrchestratorError> {
    // A non-directory `.github` cannot hold a template; report None and
    // let the replacement step deliver its own verdict on the target.
    if let Ok(meta) = std::fs::symlink_metadata(root.join(".github"))
        && !meta.is_dir()
    {
        return Ok(None);
    }
    let path = root.join(PULL_REQUEST_TEMPLATE_PATH);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(OrchestratorError::io(
                path.display().to_string(),
                err.to_string(),
            ));
        }
    };
    if !meta.is_file() {
        return Err(OrchestratorError::UnsafePath {
            path: path.display().to_string(),
            reason: "preserved_template_not_a_file".to_owned(),
        });
    }
    if meta.len() > PRESERVED_TEMPLATE_MAX_BYTES as u64 {
        return Err(OrchestratorError::UnsafePath {
            path: path.display().to_string(),
            reason: "preserved_template_too_large".to_owned(),
        });
    }
    match std::fs::read_to_string(&path) {
        Ok(bytes) => {
            if bytes.len() > PRESERVED_TEMPLATE_MAX_BYTES {
                return Err(OrchestratorError::UnsafePath {
                    path: path.display().to_string(),
                    reason: "preserved_template_too_large".to_owned(),
                });
            }
            Ok(Some(RenderedFile {
                path: PULL_REQUEST_TEMPLATE_PATH.to_owned(),
                bytes,
            }))
        }
        Err(err) => Err(OrchestratorError::io(
            path.display().to_string(),
            err.to_string(),
        )),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("velnor-write-{test}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn preserved_template_missing_stays_missing() {
        let root = scratch("preserved_missing").expect("scratch");
        assert!(
            read_preserved_template(&root)
                .expect("missing is fine")
                .is_none()
        );
    }

    #[test]
    fn preserved_template_reads_byte_exact() {
        let root = scratch("preserved_bytes").expect("scratch");
        let dir = root.join(".github");
        std::fs::create_dir_all(&dir).expect("github dir");
        let bytes = "## PR\n\nNo marker; velnor-actions runbook mention.\n";
        std::fs::write(dir.join("PULL_REQUEST_TEMPLATE.md"), bytes).expect("template");
        let preserved = read_preserved_template(&root)
            .expect("reads")
            .expect("present");
        assert_eq!(preserved.path, ".github/PULL_REQUEST_TEMPLATE.md");
        assert_eq!(preserved.bytes, bytes);
    }

    #[test]
    fn preserved_template_rejects_nonfile_and_oversize() {
        let root = scratch("preserved_gates").expect("scratch");
        let dir = root.join(".github");
        std::fs::create_dir_all(&dir).expect("github dir");
        let at = dir.join("PULL_REQUEST_TEMPLATE.md");
        std::fs::create_dir(&at).expect("dir template");
        let err = read_preserved_template(&root).expect_err("dir fails");
        assert!(
            err.to_string().contains("preserved_template_not_a_file"),
            "{err}"
        );
        std::fs::remove_dir(&at).expect("remove dir");
        std::fs::write(&at, vec![b'x'; PRESERVED_TEMPLATE_MAX_BYTES + 1]).expect("big");
        let err = read_preserved_template(&root).expect_err("oversize fails");
        assert!(
            err.to_string().contains("preserved_template_too_large"),
            "{err}"
        );
    }
}
