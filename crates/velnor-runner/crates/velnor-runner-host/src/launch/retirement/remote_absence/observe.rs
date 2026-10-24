use std::time::Instant;

use crate::{
    config::RepositoryConnection,
    host_controller::connection_pat,
    https::HttpsTransport,
    journal::{ConnectionJournal, PostJitRetirementClaim, PreparedRetirementRemoteAbsence},
    keychain,
    launch::retirement::remote_absence::model::{
        AuthenticatedRetirementAbsence, RemoteAbsenceEvidence, RetirementRemoteAbsenceError,
        binding_digest, deadline_before_lease, unix_ms,
    },
    listen::{GITHUB_API, Secret},
    scale_set::EnsureError,
};

use super::{repository, session};

/// Observe absence only through the configured repository credential and fixed GitHub API.
pub(crate) async fn observe_retirement_runner_absence(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
    binding: &RepositoryConnection,
    deadline: Instant,
) -> Result<AuthenticatedRetirementAbsence, RetirementRemoteAbsenceError> {
    let prepared = scope
        .prepare_retirement_remote_absence(claim)
        .await
        .map_err(RetirementRemoteAbsenceError::Host)?;
    validate_claim(claim, &prepared)?;
    let parts = repository::validate_binding(scope, binding, &prepared)
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    let deadline = deadline_before_lease(deadline, claim.lease_until())
        .map_err(RetirementRemoteAbsenceError::Host)?;
    let credential = keychain::load_secret(
        binding.credential_ref.service(),
        binding.credential_ref.account(),
    )
    .map_err(RetirementRemoteAbsenceError::Host)?;
    let pat = connection_pat(&credential).map_err(RetirementRemoteAbsenceError::Host)?;
    let pat = Secret::new(pat);
    drop(credential);
    observe_with_pat(binding, prepared, &parts, &pat, deadline)
}

fn observe_with_pat(
    binding: &RepositoryConnection,
    prepared: PreparedRetirementRemoteAbsence,
    parts: &repository::RepositoryParts<'_>,
    pat: &Secret,
    deadline: Instant,
) -> Result<AuthenticatedRetirementAbsence, RetirementRemoteAbsenceError> {
    let mut transport = session::Recording::new(
        HttpsTransport::with_deadline(GITHUB_API, deadline)
            .map_err(RetirementRemoteAbsenceError::Host)?,
    );
    let operation_started_unix_ms = unix_ms().map_err(RetirementRemoteAbsenceError::Host)?;
    let before = repository::lookup_repository(&mut transport, parts, pat.expose())
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    let admin = session::register_repository(&mut transport, parts, pat.expose())
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    let service_base_sha256 = binding_digest(admin.expose_url().as_bytes());
    transport
        .set_base(admin.expose_url())
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    let receipt = session::confirm_runner_absence(&mut transport, binding, &prepared, &admin)
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    transport
        .set_base(GITHUB_API)
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    let after = repository::lookup_repository(&mut transport, parts, pat.expose())
        .map_err(RetirementRemoteAbsenceError::Remote)?;
    if before != after {
        return Err(RetirementRemoteAbsenceError::Remote(EnsureError::Rejected));
    }
    let operation_finished_unix_ms = unix_ms().map_err(RetirementRemoteAbsenceError::Host)?;
    Ok(AuthenticatedRetirementAbsence::verified(
        prepared,
        RemoteAbsenceEvidence {
            repository_id: after.id(),
            repository_owner: after.owner().to_owned(),
            repository_name: after.name().to_owned(),
            service_base_sha256,
            operation_started_unix_ms,
            operation_finished_unix_ms,
            receipt,
        },
    ))
}

fn validate_claim(
    claim: &PostJitRetirementClaim,
    prepared: &PreparedRetirementRemoteAbsence,
) -> Result<(), RetirementRemoteAbsenceError> {
    if claim.reservation_id() != prepared.reservation_id()
        || claim.generation() != prepared.generation()
        || claim.claim_generation() != prepared.claim_generation()
        || claim.runner_id() != prepared.runner_id()
        || claim.runner_name() != prepared.runner_name()
    {
        return Err(RetirementRemoteAbsenceError::Remote(EnsureError::Rejected));
    }
    Ok(())
}
