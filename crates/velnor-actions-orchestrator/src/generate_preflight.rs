// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::UnmanagedPolicy;

use crate::OrchestratorError;
use crate::prepare::GenerationPreparation;

pub(super) fn fail_on_blocking_findings(
    prep: &GenerationPreparation,
) -> Result<(), OrchestratorError> {
    let blockers = crate::evidence::blocking_findings(&prep.discovery.workspaces);
    if !blockers.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "{}: {}",
                velnor_actions_rust::TRANSIENT_EVIDENCE_CODE,
                blockers.join("; ")
            ),
        });
    }
    if !prep.lock_audit_blocking.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "lock_audit_blocked: {}",
                prep.lock_audit_blocking.join("; ")
            ),
        });
    }
    Ok(())
}

pub(super) fn unmanaged_notices(
    prep: &GenerationPreparation,
) -> Result<Vec<String>, OrchestratorError> {
    let github = &prep.config.github;
    if github.unmanaged == UnmanagedPolicy::Allow {
        return Ok(Vec::new());
    }
    let entries =
        super::unmanaged::list(&prep.root, github)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    if github.unmanaged == UnmanagedPolicy::Deny {
        return Err(OrchestratorError::Contract {
            problem: format!("unmanaged_github_entries:{}", entries.join(",")),
        });
    }
    Ok(entries
        .into_iter()
        .map(|entry| format!("unmanaged_github_entry:{entry}"))
        .collect())
}

pub(super) fn reject_unmarked_workflows(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    reject_unmarked_workflows_at(&prep.root.join(".github"))
}

pub(super) fn reject_unmarked_workflows_at(github: &std::path::Path) -> Result<(), OrchestratorError> {
    super::reject_symlink(github)?;
    let path = github.join("workflows");
    super::reject_symlink(&path)?;
    let entries = match std::fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(OrchestratorError::io(path.display().to_string(), error.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
        let file = entry.path();
        let metadata = std::fs::symlink_metadata(&file).map_err(|error| OrchestratorError::io(file.display().to_string(), error.to_string()))?;
        if !metadata.is_file() || metadata.is_symlink() || !super::preserve::first_line_is_generated_marker(&file)? {
            return Err(OrchestratorError::Contract { problem: format!("unmanaged_workflow:{}", file.display()) });
        }
    }
    Ok(())
}
