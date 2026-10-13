//! Tool-file snapshots: capture at intake, verify before writes.
//!
//! Moved from the generate guards: inventory captures the snapshot and
//! generation verifies it, so the type lives in discovery (below both).

use std::path::Path;

use velnor_actions_orchestrator_core::OrchestratorError;

/// Read-only tool files generation must never modify (TOOL-2.10).
const TOOL_FILES: [&str; 6] = [
    "mise.toml",
    ".mise.toml",
    "mise.lock",
    ".mise.lock",
    ".mise-version",
    "rust-toolchain.toml",
];

/// Byte snapshot of the read-only tool files for drift detection.
///
/// Captured when generation starts and verified before any output
/// replacement, so a mid-generation tool-file change fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSnapshot {
    /// One entry per [`TOOL_FILES`] path: bytes, or `None` when absent.
    entries: Vec<(String, Option<Vec<u8>>)>,
    /// Tool files present but unreadable: never merged with missing.
    unreadable: Vec<String>,
}

impl ToolSnapshot {
    /// Capture the current tool-file bytes under `root`.
    #[must_use]
    pub fn capture(root: &Path) -> Self {
        let mut unreadable = Vec::new();
        let entries = TOOL_FILES
            .iter()
            .map(|rel| {
                let bytes = match std::fs::read(root.join(rel)) {
                    Ok(bytes) => Some(bytes),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => {
                        unreadable.push((*rel).to_owned());
                        None
                    }
                };
                ((*rel).to_owned(), bytes)
            })
            .collect();
        Self {
            entries,
            unreadable,
        }
    }

    /// Fail when any tool file differs from the captured bytes.
    ///
    /// Unreadable files fail closed: the no-write proof needs contents.
    ///
    /// # Errors
    ///
    /// Returns a contract error naming the first drifted file.
    pub fn verify(&self, root: &Path) -> Result<(), OrchestratorError> {
        let fresh = Self::capture(root);
        let mut bad = self.unreadable.clone();
        bad.extend(fresh.unreadable.iter().cloned());
        if let Some(first) = bad.iter().min() {
            return Err(OrchestratorError::Contract {
                problem: format!("tool_files_unreadable:{first}"),
            });
        }
        for ((rel, want), (_, got)) in self.entries.iter().zip(fresh.entries.iter()) {
            if want != got {
                return Err(OrchestratorError::Contract {
                    problem: format!("tool_files_changed:{rel}"),
                });
            }
        }
        Ok(())
    }
}
