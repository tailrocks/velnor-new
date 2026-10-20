use crate::error::HostError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct CapacitySettlementIdentity<'a> {
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    metadata_generation: i64,
    probe_operation_id: &'a str,
}

pub(super) async fn settlement_evidence(
    connection: &turso::Connection,
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    metadata_generation: i64,
    probe_operation_id: &str,
) -> Result<String, HostError> {
    let settlement = read_settlement(connection, reservation_id, metadata_generation).await?;
    let observed_at = settlement[5].clone();
    let outcome = settlement[6].clone();
    let pools = read_pools(connection, reservation_id, metadata_generation).await?;
    hash_json(&settlement_evidence_value(
        &CapacitySettlementIdentity {
            lineage_id,
            generation,
            reservation_id,
            intent_id,
            metadata_generation,
            probe_operation_id,
        },
        &observed_at,
        &outcome,
        &pools,
    ))
}

fn settlement_evidence_value(
    identity: &CapacitySettlementIdentity<'_>,
    observed_at: &Value,
    outcome: &Value,
    pools: &[Value],
) -> Value {
    json!([
        "velnor-post-jit-capacity-settled-v1",
        identity.lineage_id,
        identity.generation,
        identity.reservation_id,
        identity.intent_id,
        identity.metadata_generation,
        identity.probe_operation_id,
        observed_at,
        outcome,
        pools,
    ])
}

pub(super) async fn terminal_digest(
    connection: &turso::Connection,
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    claim_generation: i64,
    metadata_generation: i64,
    cleanup_probe_operation_id: &str,
) -> Result<String, HostError> {
    let claim = read_claim(connection, reservation_id, claim_generation).await?;
    let remote = read_remote(connection, reservation_id, claim_generation).await?;
    let cleanup = read_cleanup(connection, reservation_id, cleanup_probe_operation_id).await?;
    let metadata = read_settlement(connection, reservation_id, metadata_generation).await?;
    let phases = read_phases(connection, reservation_id, claim_generation).await?;
    let pools = read_pools(connection, reservation_id, metadata_generation).await?;
    let mut canonical = vec![json!("velnor-post-jit-retirement-v1")];
    canonical.extend([json!([
        lineage_id,
        generation,
        reservation_id,
        claim[3],
        claim_generation
    ])]);
    canonical.extend([json!([
        claim[5], claim[6], claim[7], claim[8], claim[9], claim[10]
    ])]);
    canonical.extend([json!([
        claim[11], claim[12], claim[13], claim[14], claim[15], claim[16], claim[17], claim[18]
    ])]);
    canonical.extend([
        remote,
        cleanup,
        metadata,
        Value::Array(phases),
        Value::Array(pools),
    ]);
    hash_json(&Value::Array(canonical))
}

async fn read_claim(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<Vec<Value>, HostError> {
    let mut rows = connection.query(
        "SELECT lineage_id,generation,reservation_id,intent_id,claim_generation,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,engine_id,docker_root_digest,instance_binding_digest,runner_id,runner_name,snapshot_sha256,created_at,lease_until FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 AND claim_generation=?2",
        (reservation_id, claim_generation),
    ).await.map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let values = vec![
        json!(row.get::<i64>(0).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(1).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(2).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(3).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(4).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(5).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(6).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(7).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(8).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(9).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(10).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(11).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(12).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(13).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(14).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(15).map_err(|_| HostError::Journal)?),
        json!(row.get::<String>(16).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(17).map_err(|_| HostError::Journal)?),
        json!(row.get::<i64>(18).map_err(|_| HostError::Journal)?),
    ];
    ensure_single(&mut rows).await?;
    Ok(values)
}

async fn read_remote(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<Value, HostError> {
    let mut rows = connection.query(
        "SELECT runner_id,runner_name,scale_set_id,lookup_status,lookup_count,lookup_value_count,request_sha256,response_sha256,remove_status,observed_at_unix_ms FROM physical_attempt_post_jit_retirement_remote_absence WHERE reservation_id=?1 AND claim_generation=?2",
        (reservation_id, claim_generation),
    ).await.map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let value = json!([
        row.get::<i64>(0).map_err(|_| HostError::Journal)?,
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        row.get::<i64>(3).map_err(|_| HostError::Journal)?,
        row.get::<i64>(4).map_err(|_| HostError::Journal)?,
        row.get::<i64>(5).map_err(|_| HostError::Journal)?,
        row.get::<String>(6).map_err(|_| HostError::Journal)?,
        row.get::<String>(7).map_err(|_| HostError::Journal)?,
        row.get::<Option<i64>>(8).map_err(|_| HostError::Journal)?,
        row.get::<i64>(9).map_err(|_| HostError::Journal)?,
    ]);
    ensure_single(&mut rows).await?;
    Ok(value)
}

async fn read_cleanup(
    connection: &turso::Connection,
    reservation_id: i64,
    probe_operation_id: &str,
) -> Result<Value, HostError> {
    let mut rows = connection.query(
        "SELECT w.binding_sha256,w.guest_boot_id,cleanup.role_mask,cleanup.runner_container_id,cleanup.dind_container_id,cleanup.shared_run_volume,cleanup.runner_home_volume,cleanup.runner_work_volume,cleanup.dind_data_volume,cleanup.metadata_volume,cleanup.containers_absent,cleanup.volumes_absent,cleanup.mounts_detached,cleanup.cgroups_absent FROM capacity_reservation_provisioning_cleanups cleanup JOIN capacity_reservation_cleanup_observer_witnesses w ON w.reservation_id=cleanup.reservation_id JOIN capacity_reservation_cleanup_observer_probes cp ON cp.reservation_id=w.reservation_id AND cp.connection_id=w.connection_id AND cp.operation_id=w.operation_id AND cp.binding_sha256=w.binding_sha256 WHERE cleanup.reservation_id=?1 AND cp.probe_operation_id=?2",
        (reservation_id, probe_operation_id),
    ).await.map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let value = json!([
        probe_operation_id,
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        row.get::<String>(3).map_err(|_| HostError::Journal)?,
        row.get::<String>(4).map_err(|_| HostError::Journal)?,
        row.get::<String>(5).map_err(|_| HostError::Journal)?,
        row.get::<String>(6).map_err(|_| HostError::Journal)?,
        row.get::<String>(7).map_err(|_| HostError::Journal)?,
        row.get::<String>(8).map_err(|_| HostError::Journal)?,
        row.get::<Option<String>>(9)
            .map_err(|_| HostError::Journal)?,
        row.get::<i64>(10).map_err(|_| HostError::Journal)?,
        row.get::<i64>(11).map_err(|_| HostError::Journal)?,
        row.get::<i64>(12).map_err(|_| HostError::Journal)?,
        row.get::<i64>(13).map_err(|_| HostError::Journal)?,
    ]);
    ensure_single(&mut rows).await?;
    Ok(value)
}

async fn read_settlement(
    connection: &turso::Connection,
    reservation_id: i64,
    metadata_generation: i64,
) -> Result<Value, HostError> {
    let mut rows = connection.query(
        "SELECT settlement.generation,claim.instance_id,settlement.probe_operation_id,settlement.profile_generation_id,settlement.profile_digest,settlement.observed_at_unix_ms,settlement.outcome FROM capacity_reservation_metadata_settlements settlement JOIN capacity_reservation_metadata_settlement_claims claim ON claim.reservation_id=settlement.reservation_id AND claim.generation=settlement.generation AND claim.probe_operation_id=settlement.probe_operation_id WHERE settlement.reservation_id=?1 AND settlement.generation=?2",
        (reservation_id, metadata_generation),
    ).await.map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let value = json!([
        row.get::<i64>(0).map_err(|_| HostError::Journal)?,
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        row.get::<String>(2).map_err(|_| HostError::Journal)?,
        row.get::<String>(3).map_err(|_| HostError::Journal)?,
        row.get::<String>(4).map_err(|_| HostError::Journal)?,
        row.get::<i64>(5).map_err(|_| HostError::Journal)?,
        row.get::<String>(6).map_err(|_| HostError::Journal)?,
    ]);
    ensure_single(&mut rows).await?;
    Ok(value)
}

async fn read_phases(
    connection: &turso::Connection,
    reservation_id: i64,
    claim_generation: i64,
) -> Result<Vec<Value>, HostError> {
    let mut rows = connection.query(
        "SELECT phase_index,phase,evidence_sha256,created_at FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 ORDER BY phase_index",
        (reservation_id, claim_generation),
    ).await.map_err(|_| HostError::Journal)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        values.push(json!([
            row.get::<i64>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<String>(2).map_err(|_| HostError::Journal)?,
            row.get::<i64>(3).map_err(|_| HostError::Journal)?,
        ]));
    }
    Ok(values)
}

async fn read_pools(
    connection: &turso::Connection,
    reservation_id: i64,
    metadata_generation: i64,
) -> Result<Vec<Value>, HostError> {
    let mut rows = connection.query(
        "SELECT pool_id,filesystem_instance_digest,medium,filesystem_kind,inode_policy,total_bytes,available_bytes,total_inodes,available_inodes FROM capacity_reservation_metadata_settlement_pools WHERE reservation_id=?1 AND generation=?2 ORDER BY pool_id",
        (reservation_id, metadata_generation),
    ).await.map_err(|_| HostError::Journal)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        values.push(json!([
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<String>(2).map_err(|_| HostError::Journal)?,
            row.get::<String>(3).map_err(|_| HostError::Journal)?,
            row.get::<String>(4).map_err(|_| HostError::Journal)?,
            row.get::<i64>(5).map_err(|_| HostError::Journal)?,
            row.get::<i64>(6).map_err(|_| HostError::Journal)?,
            row.get::<i64>(7).map_err(|_| HostError::Journal)?,
            row.get::<i64>(8).map_err(|_| HostError::Journal)?,
        ]));
    }
    if values.is_empty() {
        return Err(HostError::Journal);
    }
    Ok(values)
}

async fn ensure_single(rows: &mut turso::Rows) -> Result<(), HostError> {
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn hash_json(value: &Value) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostError::Journal)?;
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(result, "{byte:02x}").map_err(|_| HostError::Journal)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{CapacitySettlementIdentity, hash_json, settlement_evidence_value};
    use serde_json::json;

    #[test]
    fn capacity_settlement_evidence_has_versioned_fixed_bytes() {
        let observed = json!(6);
        let outcome = json!("settled");
        let pools = vec![json!(["p", "d", "disk", "btrfs", "finite", 7, 6, 5, 4])];
        let value = settlement_evidence_value(
            &CapacitySettlementIdentity {
                lineage_id: 1,
                generation: 2,
                reservation_id: 3,
                intent_id: 4,
                metadata_generation: 5,
                probe_operation_id: "probe",
            },
            &observed,
            &outcome,
            &pools,
        );
        let bytes =
            serde_json::to_vec(&value).expect("The program must convert the JSON value to bytes.");
        assert_eq!(
            bytes,
            br#"["velnor-post-jit-capacity-settled-v1",1,2,3,4,5,"probe",6,"settled",[["p","d","disk","btrfs","finite",7,6,5,4]]]"#
        );
        assert_eq!(
            hash_json(&value).expect("The program must calculate the Canonical JSON digest."),
            "6729950d5b72aca5def26d428e2e2a5c11f481be5b5b3b434d3bfb28d419b3d3"
        );
        let changed_pools = vec![json!(["p", "d", "disk", "btrfs", "finite", 7, 5, 5, 4])];
        let changed = settlement_evidence_value(
            &CapacitySettlementIdentity {
                lineage_id: 1,
                generation: 2,
                reservation_id: 3,
                intent_id: 4,
                metadata_generation: 5,
                probe_operation_id: "probe",
            },
            &observed,
            &outcome,
            &changed_pools,
        );
        assert_ne!(hash_json(&value), hash_json(&changed));
    }
}
