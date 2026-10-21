//! Authenticate exact runner absence through this host's configured authority.

mod model;
mod observe;
mod repository;
mod session;
#[cfg(test)]
mod tests;

pub(crate) use model::AuthenticatedRetirementAbsence;
pub(crate) use observe::observe_retirement_runner_absence;

#[cfg(test)]
pub(crate) fn test_authenticated_absence(
    prepared: crate::journal::PreparedRetirementRemoteAbsence,
    receipt: velnor_runner_github::RunnerAbsenceReceipt,
    repository_id: i64,
    repository_owner: String,
    repository_name: String,
) -> Result<AuthenticatedRetirementAbsence, crate::error::HostError> {
    let started = receipt
        .responses()
        .first()
        .ok_or(crate::error::HostError::Identity)?
        .observed_at_unix_ms()
        .saturating_sub(1_000);
    let finished = receipt
        .responses()
        .last()
        .ok_or(crate::error::HostError::Identity)?
        .observed_at_unix_ms()
        .checked_add(1_000)
        .ok_or(crate::error::HostError::Identity)?;
    Ok(model::AuthenticatedRetirementAbsence::verified(
        prepared,
        model::RemoteAbsenceEvidence {
            repository_id,
            repository_owner,
            repository_name,
            service_base_sha256: "a".repeat(64),
            operation_started_unix_ms: started,
            operation_finished_unix_ms: finished,
            receipt,
        },
    ))
}
