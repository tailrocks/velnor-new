use crate::error::HostError;
use crate::journal::connection::RetirementDigestIdentity;

mod digest;

pub(in crate::journal) async fn validate_retirement_digest(
    connection: &turso::Connection,
    identity: RetirementDigestIdentity<'_>,
    proof_digest: &str,
    settlement_digest: &str,
) -> Result<bool, HostError> {
    let intent_id = read_claim_intent(
        connection,
        identity.reservation_id,
        identity.claim_generation,
    )
    .await?;
    let expected_settlement = digest::settlement_evidence(
        connection,
        identity.lineage_id,
        identity.generation,
        identity.reservation_id,
        intent_id,
        identity.metadata_generation,
        identity.cleanup_probe_operation_id,
    )
    .await?;
    let stored_settlement = read_capacity_settled(
        connection,
        identity.reservation_id,
        identity.claim_generation,
    )
    .await?;
    let expected_terminal = digest::terminal_digest(
        connection,
        identity.lineage_id,
        identity.generation,
        identity.reservation_id,
        identity.claim_generation,
        identity.metadata_generation,
        identity.cleanup_probe_operation_id,
    )
    .await?;
    Ok(stored_settlement == expected_settlement
        && proof_digest == expected_terminal
        && settlement_digest == expected_terminal)
}

async fn read_claim_intent(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 AND claim_generation=?2",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let intent_id = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(intent_id)
}

async fn read_capacity_settled(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT evidence_sha256 FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=5 AND phase='capacity_settled'",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let evidence = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(evidence)
}

pub(super) async fn settle_if_claimed(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let Some(claim_generation) =
        latest_active_claim(connection, connection_id, intent_id, reservation_id).await?
    else {
        return Ok(false);
    };
    let (lineage_id, generation, metadata_generation, cleanup_probe_operation_id) =
        retirement_identity(connection, reservation_id, claim_generation).await?;
    let evidence = digest::settlement_evidence(
        connection,
        lineage_id,
        generation,
        reservation_id,
        intent_id,
        metadata_generation,
        cleanup_probe_operation_id.as_str(),
    )
    .await?;
    insert_capacity_settled(
        connection,
        reservation_id,
        claim_generation,
        evidence.as_str(),
    )
    .await?;
    let terminal_digest = digest::terminal_digest(
        connection,
        lineage_id,
        generation,
        reservation_id,
        claim_generation,
        metadata_generation,
        cleanup_probe_operation_id.as_str(),
    )
    .await?;
    insert_terminal(
        connection,
        TerminalRow {
            lineage_id,
            generation,
            reservation_id,
            claim_generation,
            metadata_generation,
            cleanup_probe_operation_id,
            digest: terminal_digest,
        },
    )
    .await?;
    release_and_remove_current(
        connection,
        connection_id,
        intent_id,
        reservation_id,
        lineage_id,
        generation,
    )
    .await?;
    Ok(true)
}

async fn latest_active_claim(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
    reservation_id: i64,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*), MAX(claim_generation) FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let count: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let latest: Option<i64> = row.get(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    if count == 0 {
        return Ok(None);
    }
    let generation = latest.ok_or(HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT 1 FROM physical_attempt_post_jit_retirement_claims claim JOIN physical_attempt_current current ON current.lineage_id=claim.lineage_id AND current.generation=claim.generation AND current.reservation_id=claim.reservation_id AND current.intent_id=claim.intent_id WHERE claim.reservation_id=?1 AND claim.intent_id=?2 AND claim.connection_id=?3 AND claim.claim_generation=?4 AND claim.lease_until>unixepoch()",
            (reservation_id, intent_id, connection_id, generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_none()
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(Some(generation))
}

async fn retirement_identity(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<(i64, i64, i64, String), HostError> {
    let mut rows = connection
        .query(
            "SELECT claim.lineage_id, claim.generation, settlement.generation, observer.probe_operation_id FROM physical_attempt_post_jit_retirement_claims claim JOIN capacity_reservation_metadata_settlement_claims settlement ON settlement.reservation_id=claim.reservation_id AND settlement.operation_id=claim.intent_id AND settlement.connection_id=claim.connection_id JOIN capacity_reservation_cleanup_observer_probes observer ON observer.reservation_id=claim.reservation_id AND observer.connection_id=claim.connection_id AND observer.operation_id=claim.intent_id WHERE claim.reservation_id=?1 AND claim.claim_generation=?2 AND settlement.generation=(SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims latest WHERE latest.reservation_id=claim.reservation_id)",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let identity = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(identity)
}

async fn insert_capacity_settled(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
    evidence: &str,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_progress (reservation_id, claim_generation, phase_index, phase, evidence_sha256, created_at) VALUES (?1, ?2, 5, 'capacity_settled', ?3, unixepoch())",
            (reservation_id, claim_generation, evidence),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

struct TerminalRow {
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    claim_generation: i64,
    metadata_generation: i64,
    cleanup_probe_operation_id: String,
    digest: String,
}

async fn insert_terminal(
    connection: &turso::Connection,
    terminal: TerminalRow,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirements (lineage_id,generation,reservation_id,claim_generation,metadata_generation,cleanup_probe_operation_id,proof_digest,settlement_digest,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7,unixepoch())",
            (terminal.lineage_id, terminal.generation, terminal.reservation_id, terminal.claim_generation, terminal.metadata_generation, terminal.cleanup_probe_operation_id.as_str(), terminal.digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn release_and_remove_current(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
    reservation_id: i64,
    lineage_id: i64,
    generation: i64,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET state='released' WHERE id=?1 AND connection_id=?2 AND intent_id=?3 AND state IN ('reserved','uncertain','recovering') AND remote_cleanup_proven=1 AND local_cleanup_proven=1",
            (reservation_id, connection_id, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "DELETE FROM physical_attempt_current WHERE lineage_id=?1 AND generation=?2 AND reservation_id=?3 AND intent_id=?4",
            (lineage_id, generation, reservation_id, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
