//! Enumerate `.github` entries that generation preserves as repository content.
//!
//! The walk mirrors `generate_preserve::copy_entry`: an entry that generation
//! replaces or retires is generator-owned and skipped; everything else is
//! unmanaged. Allowlisted workflows (`preserve_workflows`) and `[github] allow`
//! paths are exempt.

use std::path::{Path, PathBuf};

use velnor_actions_contract::{GithubConfig, UnmanagedPolicy};

use super::preserve::{
    generated_marked_dco, generated_marked_script, generated_shared_action, generator_owned,
};
use crate::OrchestratorError;

/// Sorted repository-relative paths (`.github/...`) of non-exempt unmanaged
/// files, symlinks and empty directories.
///
/// # Errors
///
/// Returns unsafe-path errors for a symlinked `.github` and IO errors.
pub(crate) fn list(
    root: &Path,
    preserve_workflows: &[String],
    github: &GithubConfig,
) -> Result<Vec<String>, OrchestratorError> {
    let base = root.join(".github");
    match std::fs::symlink_metadata(&base) {
        Ok(meta) if meta.is_symlink() || !meta.is_dir() => {
            return Err(OrchestratorError::UnsafePath {
                path: base.display().to_string(),
                reason: "github_not_directory".to_owned(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(&base, &error)),
    }
    let mut found = Vec::new();
    walk(&base, Path::new(""), preserve_workflows, &mut found)?;
    let mut paths: Vec<String> = found
        .into_iter()
        .map(|relative| {
            let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
            format!(".github/{}", parts.join("/"))
        })
        .filter(|path| !github.is_allowed(path))
        .filter(|path| !preserve_workflows.iter().any(|preserved| preserved == path))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Result of the `unmanaged` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedReport {
    /// Configured `[github] unmanaged` policy.
    pub policy: UnmanagedPolicy,
    /// Non-exempt unmanaged entries, sorted.
    pub entries: Vec<String>,
}

impl UnmanagedReport {
    /// True when the policy is `deny` and at least one entry is listed.
    #[must_use]
    pub fn is_denied(&self) -> bool {
        self.policy == UnmanagedPolicy::Deny && !self.entries.is_empty()
    }
}

/// Read `.velnor/config.toml` and list unmanaged `.github` entries regardless
/// of the configured policy (the allowlists still apply).
///
/// # Errors
///
/// Returns config, unsafe-path, or IO errors.
pub fn unmanaged_report(root: &Path) -> Result<UnmanagedReport, OrchestratorError> {
    let config = crate::config::load_config(root)?;
    let entries = list(root, &config.workflow.preserve_workflows, &config.github)?;
    Ok(UnmanagedReport {
        policy: config.github.unmanaged,
        entries,
    })
}

/// Collect unmanaged leaves below `relative`, without following links.
fn walk(
    base: &Path,
    relative: &Path,
    preserve_workflows: &[String],
    found: &mut Vec<PathBuf>,
) -> Result<bool, OrchestratorError> {
    let source = base.join(relative);
    let metadata = std::fs::symlink_metadata(&source).map_err(|error| io(&source, &error))?;
    if !relative.as_os_str().is_empty()
        && (generator_owned(relative, preserve_workflows)
            || generated_shared_action(relative, &source, &metadata)?
            || generated_marked_script(relative, &source, &metadata)?
            || generated_marked_dco(relative, &source, &metadata)?)
    {
        return Ok(false);
    }
    if !metadata.is_dir() || metadata.is_symlink() {
        found.push(relative.to_path_buf());
        return Ok(true);
    }
    let mut had_entry = false;
    let mut kept = false;
    for entry in std::fs::read_dir(&source).map_err(|error| io(&source, &error))? {
        let entry = entry.map_err(|error| io(&source, &error))?;
        had_entry = true;
        kept |= walk(
            base,
            &relative.join(entry.file_name()),
            preserve_workflows,
            found,
        )?;
    }
    if !had_entry && !relative.as_os_str().is_empty() {
        found.push(relative.to_path_buf());
        return Ok(true);
    }
    Ok(kept)
}

fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

#[cfg(test)]
#[path = "generate_unmanaged_tests.rs"]
mod tests;
