use serde_json::json;
use velnor_runner_github::{RunnerObservationOperation, RunnerRemovalStatus};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::retirement::AuthenticatedRetirementAbsence;

use super::claim::PostJitRetirementClaim;
use super::identity::hex_digest;
use super::progress::{append_phase, validate_in_transaction};

pub(super) async fn record_remote_absence(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
    absence: &AuthenticatedRetirementAbsence,
) -> Result<(), HostError> {
    let identity = claim.identity();
    validate_authenticated_absence(claim, absence)?;
    let (remove_status, request_digest, response_digest, observed_at) =
        validate_receipt(claim, absence)?;
    let observed_at = i64::try_from(observed_at).map_err(|_| HostError::Identity)?;
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        validate_in_transaction(connection, scope, claim).await?;
        validate_prepared_phase1(connection, claim, absence).await?;
        let mut rows = connection
            .query(
                "SELECT created_at FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 AND claim_generation=?2 AND snapshot_sha256=?3 AND lease_until>unixepoch()",
                (
                    identity.reservation_id,
                    claim.claim_generation(),
                    claim.snapshot_sha256(),
                ),
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
    })
    .await
}

#[cfg(test)]
#[path = "remote/mock.rs"]
mod mock;
#[cfg(test)]
pub(super) use mock::record_mocked_remote_absence;

#[cfg(test)]
async fn validate_phase1(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT phase FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=1",
            (claim.reservation_id(), claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let phase: String = row.get(0).map_err(|_| HostError::Journal)?;
    if phase != "runner_stopped" || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_receipt<'a>(
    claim: &PostJitRetirementClaim,
    absence: &'a AuthenticatedRetirementAbsence,
) -> Result<(Option<i64>, &'a str, &'a str, u128), HostError> {
    let identity = claim.identity();
    let receipt = absence.receipt();
    let runner = receipt.runner();
    if runner.id != identity.runner_id
        || runner.name != identity.runner_name
        || runner.runner_scale_set_id != identity.scale_set_id
    {
        return Err(HostError::Identity);
    }
    let responses = receipt.responses();
    let (remove_status, final_lookup) = match receipt.removal_status() {
        RunnerRemovalStatus::NotNeeded if responses.len() == 1 => {
            let lookup = &responses[0];
            if lookup.operation() != RunnerObservationOperation::LookupByName
                || lookup.status() != Some(200)
                || !lookup.status_verified()
                || lookup.count() != Some(0)
                || lookup.value_count() != Some(0)
            {
                return Err(HostError::Identity);
            }
            (None, lookup)
        }
        RunnerRemovalStatus::Removed if responses.len() == 3 => {
            let initial = &responses[0];
            let remove = &responses[1];
            if initial.operation() != RunnerObservationOperation::LookupByName
                || initial.status() != Some(200)
                || !initial.status_verified()
                || initial.count() != Some(1)
                || initial.value_count() != Some(1)
                || remove.operation() != RunnerObservationOperation::RemoveById
                || remove.status() != Some(204)
                || !remove.status_verified()
                || remove.response_sha256().is_none()
            {
                return Err(HostError::Identity);
            }
            (Some(204), &responses[2])
        }
        RunnerRemovalStatus::AbsentAfterRejectedRemoval
        | RunnerRemovalStatus::AbsentAfterUncertainRemoval
        | RunnerRemovalStatus::NotNeeded
        | RunnerRemovalStatus::Removed => return Err(HostError::Identity),
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
    let observed_at = final_lookup.observed_at_unix_ms();
    if !valid_digest(request_digest)
        || !valid_digest(response_digest)
        || observed_at < absence.operation_started_unix_ms()
        || observed_at > absence.operation_finished_unix_ms()
    {
        return Err(HostError::Identity);
    }
    Ok((remove_status, request_digest, response_digest, observed_at))
}

fn validate_authenticated_absence(
    claim: &PostJitRetirementClaim,
    absence: &AuthenticatedRetirementAbsence,
) -> Result<(), HostError> {
    let identity = claim.identity();
    let prepared = absence.prepared();
    if prepared.reservation_id() != identity.reservation_id
        || prepared.generation() != identity.generation
        || prepared.claim_generation() != claim.claim_generation()
        || prepared.snapshot_sha256() != claim.snapshot_sha256()
        || prepared.connection_id() != identity.connection_id
        || prepared.binding_digest() != identity.binding_digest
        || prepared.scale_set_id() != identity.scale_set_id
        || prepared.runner_id() != identity.runner_id
        || prepared.runner_name() != identity.runner_name
        || prepared.phase1_evidence_sha256().len() != 64
        || absence.repository_id() <= 0
        || absence.repository_owner().is_empty()
        || absence.repository_name().is_empty()
        || !absence.repository_private()
        || !valid_digest(absence.service_base_sha256())
        || absence.operation_started_unix_ms() == 0
        || absence.operation_finished_unix_ms() < absence.operation_started_unix_ms()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn validate_prepared_phase1(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
    absence: &AuthenticatedRetirementAbsence,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT phase,evidence_sha256 FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=1",
            (claim.reservation_id(), claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let phase: String = row.get(0).map_err(|_| HostError::Journal)?;
    let evidence: String = row.get(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || phase != "runner_stopped"
        || evidence != absence.prepared().phase1_evidence_sha256()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn insert_or_match_receipt(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
    remove_status: Option<i64>,
    request_digest: &str,
    response_digest: &str,
    observed_at: i64,
) -> Result<(), HostError> {
    let identity = claim.identity();
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_remote_absence (reservation_id,claim_generation,runner_id,runner_name,scale_set_id,lookup_status,lookup_count,lookup_value_count,request_sha256,response_sha256,remove_status,observed_at_unix_ms) VALUES (?1,?2,?3,?4,?5,200,0,0,?6,?7,?8,?9) ON CONFLICT(reservation_id,claim_generation) DO NOTHING",
            turso::params![
                identity.reservation_id,
                claim.claim_generation(),
                identity.runner_id,
                identity.runner_name.as_str(),
                identity.scale_set_id,
                request_digest,
                response_digest,
                remove_status,
                observed_at,
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT runner_id,runner_name,scale_set_id,lookup_status,lookup_count,lookup_value_count,request_sha256,response_sha256,remove_status,observed_at_unix_ms FROM physical_attempt_post_jit_retirement_remote_absence WHERE reservation_id=?1 AND claim_generation=?2",
            (identity.reservation_id, claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let matches = row.get::<i64>(0).map_err(|_| HostError::Journal)? == identity.runner_id
        && row.get::<String>(1).map_err(|_| HostError::Journal)? == identity.runner_name
        && row.get::<i64>(2).map_err(|_| HostError::Journal)? == identity.scale_set_id
        && row.get::<i64>(3).map_err(|_| HostError::Journal)? == 200
        && row.get::<i64>(4).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(5).map_err(|_| HostError::Journal)? == 0
        && row.get::<String>(6).map_err(|_| HostError::Journal)? == request_digest
        && row.get::<String>(7).map_err(|_| HostError::Journal)? == response_digest
        && row.get::<Option<i64>>(8).map_err(|_| HostError::Journal)? == remove_status
        && row.get::<i64>(9).map_err(|_| HostError::Journal)? == observed_at
        && rows.next().await.map_err(|_| HostError::Journal)?.is_none();
    if matches {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn remote_evidence(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
    remove_status: Option<i64>,
    request_digest: &str,
    response_digest: &str,
    observed_at: i64,
) -> Result<String, HostError> {
    let identity = claim.identity();
    let value = json!([
        "velnor-post-jit-retirement-remote-absence-v1",
        identity.reservation_id,
        identity.generation,
        identity.connection_id,
        identity.scale_set_id,
        claim.claim_generation(),
        identity.runner_id,
        identity.runner_name,
        request_digest,
        response_digest,
        remove_status,
        observed_at,
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
    let evidence = hex_digest(&bytes);
    let mut rows = connection
        .query(
            "SELECT request_sha256,response_sha256,remove_status,observed_at_unix_ms FROM physical_attempt_post_jit_retirement_remote_absence WHERE reservation_id=?1 AND claim_generation=?2",
            (identity.reservation_id, claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if row.get::<String>(0).map_err(|_| HostError::Journal)? != request_digest
        || row.get::<String>(1).map_err(|_| HostError::Journal)? != response_digest
        || row.get::<Option<i64>>(2).map_err(|_| HostError::Journal)? != remove_status
        || row.get::<i64>(3).map_err(|_| HostError::Journal)? != observed_at
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Identity);
    }
    Ok(evidence)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
