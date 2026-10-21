use velnor_runner_github::{RunnerAbsenceReceipt, RunnerObservationOperation, RunnerRemovalStatus};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;

use super::{
    PostJitRetirementClaim, append_phase, insert_or_match_receipt, remote_evidence, valid_digest,
    validate_in_transaction, validate_phase1,
};

/// Persist external evidence supplied by a mocked transport in storage-only tests.
pub(crate) async fn record_mocked_remote_absence(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
    receipt: &RunnerAbsenceReceipt,
) -> Result<(), HostError> {
    let (remove_status, request_digest, response_digest, observed_at) =
        validated_mock_receipt(claim, receipt)?;
    in_transaction(scope.journal_ref(), async |connection| {
        record_mocked_evidence(
            connection,
            scope,
            claim,
            remove_status,
            request_digest,
            response_digest,
            observed_at,
        )
        .await
    })
    .await
}

fn validated_mock_receipt<'a>(
    claim: &PostJitRetirementClaim,
    receipt: &'a RunnerAbsenceReceipt,
) -> Result<(Option<i64>, &'a str, &'a str, i64), HostError> {
    let identity = claim.identity();
    let runner = receipt.runner();
    if runner.id != identity.runner_id
        || runner.name != identity.runner_name
        || runner.runner_scale_set_id != identity.scale_set_id
    {
        return Err(HostError::Identity);
    }
    let responses = receipt.responses();
    let (remove_status, final_lookup) = match receipt.removal_status() {
        RunnerRemovalStatus::NotNeeded if responses.len() == 1 => (None, &responses[0]),
        RunnerRemovalStatus::Removed if responses.len() == 3 => (Some(204), &responses[2]),
        _ => return Err(HostError::Identity),
    };
    if final_lookup.operation() != RunnerObservationOperation::LookupByName
        || final_lookup.status() != Some(200)
        || !final_lookup.status_verified()
        || final_lookup.count() != Some(0)
        || final_lookup.value_count() != Some(0)
    {
        return Err(HostError::Identity);
    }
    let request_digest = final_lookup.request_sha256();
    let response_digest = final_lookup.response_sha256().ok_or(HostError::Identity)?;
    let observed_at =
        i64::try_from(final_lookup.observed_at_unix_ms()).map_err(|_| HostError::Identity)?;
    if !valid_digest(request_digest) || !valid_digest(response_digest) {
        return Err(HostError::Identity);
    }
    Ok((remove_status, request_digest, response_digest, observed_at))
}

async fn record_mocked_evidence(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
    remove_status: Option<i64>,
    request_digest: &str,
    response_digest: &str,
    observed_at: i64,
) -> Result<(), HostError> {
    let identity = claim.identity();
    scope.validate_scope(connection).await?;
    validate_in_transaction(connection, scope, claim).await?;
    validate_phase1(connection, claim).await?;
    let mut rows = connection
            .query(
                "SELECT created_at FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 AND claim_generation=?2 AND snapshot_sha256=?3 AND lease_until>unixepoch()",
                (identity.reservation_id, claim.claim_generation(), claim.snapshot_sha256()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let created_at: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    if observed_at < created_at.checked_mul(1000).ok_or(HostError::Identity)?
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Identity);
    }
    insert_or_match_receipt(
        connection,
        claim,
        remove_status,
        request_digest,
        response_digest,
        observed_at,
    )
    .await?;
    let updated = connection
            .execute(
                "UPDATE capacity_reservations SET remote_cleanup_proven=1 WHERE id=?1 AND connection_id=?2 AND remote_cleanup_proven IN (0,1) AND state IN ('reserved','uncertain','recovering')",
                (identity.reservation_id, identity.connection_id.as_str()),
            )
            .await
        .map_err(|_| HostError::Journal)?;
    if updated != 1 {
        return Err(HostError::Journal);
    }
    let evidence = remote_evidence(
        connection,
        claim,
        remove_status,
        request_digest,
        response_digest,
        observed_at,
    )
    .await?;
    append_phase(connection, claim, 2, "remote_absent", &evidence).await
}
