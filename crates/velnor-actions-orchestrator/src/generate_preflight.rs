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
        super::unmanaged::list(&prep.root, &prep.config.workflow.preserve_workflows, github)?;
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
