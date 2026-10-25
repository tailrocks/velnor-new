use crate::error::HostError;
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validate_claim_identities(conn).await?;
    validate_claim_snapshots(conn).await?;
    validate_progress(conn).await
}

async fn validate_claim_identities(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims c LEFT JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id LEFT JOIN capacity_reservations r ON r.id=c.reservation_id AND r.intent_id=c.intent_id AND r.connection_id=c.connection_id LEFT JOIN connection_intents i ON i.id=c.intent_id AND i.connection_id=c.connection_id LEFT JOIN capacity_reservation_provisioning p ON p.reservation_id=c.reservation_id AND p.operation_id=c.intent_id LEFT JOIN capacity_reservation_provisioning_state s ON s.reservation_id=c.reservation_id LEFT JOIN connection_runner_jits j ON j.intent_id=c.intent_id AND j.connection_id=c.connection_id WHERE a.lineage_id IS NULL OR a.connection_id!=c.connection_id OR a.scale_set_id!=c.scale_set_id OR a.witness_request_id!=c.witness_request_id OR a.binding_digest!=c.binding_digest OR a.source_digest!=c.source_digest OR (a.acquire_response_digest IS NOT NULL AND a.acquire_response_digest!=c.acquire_response_digest) OR a.engine_id!=c.engine_id OR a.docker_root_digest!=c.docker_root_digest OR r.id IS NULL OR r.binding_digest!=c.binding_digest OR r.engine_id!=c.engine_id OR r.docker_root_digest!=c.docker_root_digest OR i.kind!='launch' OR i.scale_set_id!=c.scale_set_id OR i.runner_name!=c.runner_name OR i.github_runner_id!=CAST(c.runner_id AS TEXT) OR p.engine_id!=c.engine_id OR p.docker_root_digest!=c.docker_root_digest OR s.instance_binding_digest!=c.instance_binding_digest OR s.runner_id!=c.runner_id OR s.runner_name!=c.runner_name OR s.runner_scale_set_id!=c.scale_set_id OR j.runner_id!=c.runner_id OR j.runner_name!=c.runner_name OR j.scale_set_id!=c.scale_set_id OR j.witness_request_id!=c.witness_request_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn validate_claim_snapshots(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT lineage_id,generation,reservation_id,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,engine_id,docker_root_digest,instance_binding_digest,runner_id,runner_name,snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims ORDER BY reservation_id,claim_generation",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let lineage_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let generation = row.get::<i64>(1).map_err(|_| HostError::Journal)?;
        let reservation_id = row.get::<i64>(2).map_err(|_| HostError::Journal)?;
        let intent_id = row.get::<i64>(3).map_err(|_| HostError::Journal)?;
        let connection_id = row.get::<String>(4).map_err(|_| HostError::Journal)?;
        let scale_set_id = row.get::<i64>(5).map_err(|_| HostError::Journal)?;
        let witness_request_id = row.get::<i64>(6).map_err(|_| HostError::Journal)?;
        let binding = row.get::<String>(7).map_err(|_| HostError::Journal)?;
        let source = row.get::<String>(8).map_err(|_| HostError::Journal)?;
        let acquire = row.get::<String>(9).map_err(|_| HostError::Journal)?;
        let engine = row.get::<String>(10).map_err(|_| HostError::Journal)?;
        let root = row.get::<String>(11).map_err(|_| HostError::Journal)?;
        let instance_binding = row.get::<String>(12).map_err(|_| HostError::Journal)?;
        let runner_id = row.get::<i64>(13).map_err(|_| HostError::Journal)?;
        let runner_name = row.get::<String>(14).map_err(|_| HostError::Journal)?;
        let snapshot = row.get::<String>(15).map_err(|_| HostError::Journal)?;
        let value = json!([
            "velnor-post-jit-retirement-identity-v1",
            lineage_id,
            generation,
            reservation_id,
            intent_id,
            connection_id,
            scale_set_id,
            witness_request_id,
            binding,
            source,
            acquire,
            engine,
            root,
            instance_binding,
            runner_id,
            runner_name,
        ]);
        let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
        if digest(&bytes) != snapshot {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_progress(conn: &turso::Connection) -> Result<(), HostError> {
    let mut claims = conn
        .query(
            "SELECT reservation_id,claim_generation,snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims ORDER BY reservation_id,claim_generation",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(claim) = claims.next().await.map_err(|_| HostError::Journal)? {
        let reservation_id = claim.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let claim_generation = claim.get::<i64>(1).map_err(|_| HostError::Journal)?;
        let snapshot = claim.get::<String>(2).map_err(|_| HostError::Journal)?;
        validate_claim_progress(conn, reservation_id, claim_generation, &snapshot).await?;
    }
    Ok(())
}

async fn validate_claim_progress(
    conn: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
    snapshot: &str,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT phase_index,phase,evidence_sha256 FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 ORDER BY phase_index",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut expected_index = 0;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let index = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let phase = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let evidence = row.get::<String>(2).map_err(|_| HostError::Journal)?;
        let expected_phase = phase_name(index).ok_or(HostError::Journal)?;
        if index != expected_index || phase != expected_phase {
            return Err(HostError::Journal);
        }
        if index == 0 && evidence != snapshot {
            return Err(HostError::Journal);
        }
        if index == 5 && !has_terminal(conn, reservation_id, claim_generation).await? {
            return Err(HostError::Journal);
        }
        validate_phase_proof(conn, reservation_id, claim_generation, index).await?;
        expected_index += 1;
    }
    validate_remote_receipt(conn, reservation_id, claim_generation, expected_index).await
}

async fn validate_phase_proof(
    conn: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
    phase_index: i64,
) -> Result<(), HostError> {
    let query = match phase_index {
        2 => {
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_remote_absence r JOIN physical_attempt_post_jit_retirement_claims c ON c.reservation_id=r.reservation_id AND c.claim_generation=r.claim_generation WHERE r.reservation_id=?1 AND r.claim_generation=?2 AND r.runner_id=c.runner_id AND r.runner_name=c.runner_name AND r.scale_set_id=c.scale_set_id AND r.lookup_status=200 AND r.lookup_count=0 AND r.lookup_value_count=0 AND (r.remove_status IS NULL OR r.remove_status=204)"
        }
        3 => {
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_provisioning_state s ON s.reservation_id=cleanup.reservation_id JOIN physical_attempt_post_jit_retirement_claims c ON c.reservation_id=cleanup.reservation_id WHERE cleanup.reservation_id=?1 AND c.claim_generation=?2 AND cleanup.instance_binding_digest=c.instance_binding_digest AND s.instance_binding_digest=c.instance_binding_digest AND cleanup.runner_container_id=s.runner_container_id AND cleanup.dind_container_id=s.dind_container_id AND cleanup.role_mask=2047 AND cleanup.containers_absent=1 AND cleanup.volumes_absent=1 AND cleanup.mounts_detached=1 AND cleanup.cgroups_absent=1 AND s.storage_cleanup_proven=1"
        }
        4 => {
            "SELECT COUNT(*) FROM capacity_reservation_networks n JOIN physical_attempt_post_jit_retirement_claims c ON c.reservation_id=n.reservation_id WHERE n.reservation_id=?1 AND c.claim_generation=?2 AND n.connection_id=c.connection_id AND n.operation_id=c.intent_id AND n.engine_id=c.engine_id AND n.docker_root_digest=c.docker_root_digest AND n.create_state='created' AND n.network_id IS NOT NULL AND n.remove_state='removed'"
        }
        _ => return Ok(()),
    };
    let mut rows = conn
        .query(query, (reservation_id, claim_generation))
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if count == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn validate_remote_receipt(
    conn: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
    next_phase: i64,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_remote_absence r JOIN physical_attempt_post_jit_retirement_claims c ON c.reservation_id=r.reservation_id AND c.claim_generation=r.claim_generation LEFT JOIN physical_attempt_post_jit_retirement_progress stopped ON stopped.reservation_id=r.reservation_id AND stopped.claim_generation=r.claim_generation AND stopped.phase_index=1 AND stopped.phase='runner_stopped' WHERE r.reservation_id=?1 AND r.claim_generation=?2 AND (stopped.reservation_id IS NULL OR r.runner_id!=c.runner_id OR r.runner_name!=c.runner_name OR r.scale_set_id!=c.scale_set_id OR r.lookup_status!=200 OR r.lookup_count!=0 OR r.lookup_value_count!=0 OR (r.remove_status IS NOT NULL AND r.remove_status!=204))",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if invalid > 0
        || (next_phase >= 3 && !has_receipt(conn, reservation_id, claim_generation).await?)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn has_receipt(
    conn: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_remote_absence WHERE reservation_id=?1 AND claim_generation=?2",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}

async fn has_terminal(
    conn: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirements WHERE reservation_id=?1 AND claim_generation=?2",
            (reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}

fn phase_name(index: i64) -> Option<&'static str> {
    match index {
        0 => Some("claimed"),
        1 => Some("runner_stopped"),
        2 => Some("remote_absent"),
        3 => Some("storage_absent"),
        4 => Some("network_absent"),
        5 => Some("capacity_settled"),
        _ => None,
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _written = write!(output, "{byte:02x}");
    }
    output
}
