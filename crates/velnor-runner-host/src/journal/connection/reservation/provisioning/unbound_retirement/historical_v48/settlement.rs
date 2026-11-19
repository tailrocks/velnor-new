// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::UnboundRetirementClaim;

pub(super) async fn settle(
    connection: &turso::Connection,
    claim: &UnboundRetirementClaim,
) -> Result<(), HostError> {
    let (lineage_id, generation) = current_attempt(connection, claim).await?;
    let digest = proof_digest(connection, claim).await?;
    expect_one(
        connection,
        "INSERT INTO physical_attempt_unbound_retirements (lineage_id, generation, reservation_id, proof_digest, settlement_digest, created_at) VALUES (?1, ?2, ?3, ?4, ?4, unixepoch())",
        (lineage_id, generation, claim.reservation_id, digest.as_str()),
    )
    .await?;
    expect_one(
        connection,
        "UPDATE capacity_reservations SET state = 'released', remote_cleanup_proven = 1, local_cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND intent_id = ?3 AND state = 'uncertain' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0",
        (claim.reservation_id, claim.connection_id.as_str(), claim.operation_id),
    )
    .await?;
    expect_one(
        connection,
        "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'uncertain' AND cleanup_proven = 0",
        (claim.operation_id, claim.connection_id.as_str()),
    )
    .await?;
    expect_one(
        connection,
        "DELETE FROM physical_attempt_current WHERE lineage_id = ?1 AND generation = ?2 AND reservation_id = ?3 AND intent_id = ?4",
        (lineage_id, generation, claim.reservation_id, claim.operation_id),
    )
    .await
}

async fn current_attempt(
    connection: &turso::Connection,
    claim: &UnboundRetirementClaim,
) -> Result<(i64, i64), HostError> {
    let mut rows = connection
        .query(
            "SELECT lineage_id, generation FROM physical_attempt_current WHERE reservation_id = ?1 AND intent_id = ?2",
            (claim.reservation_id, claim.operation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    ))
}

async fn proof_digest(
    connection: &turso::Connection,
    claim: &UnboundRetirementClaim,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT phase, evidence_sha256 FROM capacity_reservation_unbound_retirement_progress WHERE reservation_id = ?1 ORDER BY phase",
            [claim.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut phases = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        phases.push(serde_json::json!({
            "phase": row.get::<String>(0).map_err(|_| HostError::Journal)?,
            "evidence_sha256": row.get::<String>(1).map_err(|_| HostError::Journal)?,
        }));
    }
    let mut rows = connection
        .query(
            "SELECT network_name, network_id, remove_state FROM capacity_reservation_networks WHERE reservation_id = ?1",
            [claim.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let network = match rows.next().await.map_err(|_| HostError::Journal)? {
        Some(row) => serde_json::json!({
            "name": row.get::<String>(0).map_err(|_| HostError::Journal)?,
            "id": row.get::<Option<String>>(1).map_err(|_| HostError::Journal)?,
            "remove_state": row.get::<String>(2).map_err(|_| HostError::Journal)?,
        }),
        None => serde_json::Value::Null,
    };
    let bytes = serde_json::to_vec(&serde_json::json!({
        "reservation_id": claim.reservation_id,
        "operation_id": claim.operation_id,
        "engine_id": claim.engine_id,
        "docker_root_digest": claim.docker_root_digest,
        "phases": phases,
        "network": network,
    }))
    .map_err(|_| HostError::Identity)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect::<String>())
}

async fn expect_one(
    connection: &turso::Connection,
    sql: &str,
    params: impl turso::params::IntoParams,
) -> Result<(), HostError> {
    let changed = connection
        .execute(sql, params)
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
