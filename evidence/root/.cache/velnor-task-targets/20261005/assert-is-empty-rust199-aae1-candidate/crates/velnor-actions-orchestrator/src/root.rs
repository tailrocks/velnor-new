//! Repository-root discovery through the Mise Git allowlist.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;

/// Resolve the canonical working-tree root from `cwd`.
///
/// Runs `git rev-parse --is-inside-work-tree` (must print `true`) and
/// `git rev-parse --show-toplevel`, then canonicalizes. All execution is
/// delegated to the Mise adapter; this module builds no child invocations.
///
/// # Errors
///
/// Returns [`OrchestratorError::NotWorkTree`] outside a working tree and
/// [`OrchestratorError::RootDiscovery`] when Git or canonicalization fails.
pub fn resolve_root(cwd: &Path) -> Result<PathBuf, OrchestratorError> {
    let inside = GitRequest::rev_parse(vec![OsString::from("--is-inside-work-tree")])
        .run_in(cwd)
        .map_err(|err| OrchestratorError::RootDiscovery {
            problem: err.to_string(),
        })?;
    if !inside.success || inside.stdout_text("git").unwrap_or_default().trim() != "true" {
        return Err(OrchestratorError::NotWorkTree {
            problem: format!("not_inside_work_tree:{}", cwd.display()),
        });
    }
    let top = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")])
        .run_in(cwd)
        .map_err(|err| OrchestratorError::RootDiscovery {
            problem: err.to_string(),
        })?;
    top.require_success("git")
        .map_err(|err| OrchestratorError::RootDiscovery {
            problem: err.to_string(),
        })?;
    let text = top
        .stdout_text("git")
        .map_err(|err| OrchestratorError::RootDiscovery {
            problem: err.to_string(),
        })?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(OrchestratorError::RootDiscovery {
            problem: "empty_toplevel".to_owned(),
        });
    }
    PathBuf::from(trimmed)
        .canonicalize()
        .map_err(|err| OrchestratorError::RootDiscovery {
            problem: format!("canonicalize:{trimmed}:{err}"),
        })
}
