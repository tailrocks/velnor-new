use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

/// Retirement status for the physical attempt that matches the event.
pub(in crate::journal) enum EventRetirementStatus {
    Claimed,
    Retired,
}

/// Identity data for a late lifecycle event. These data do not permit job execution.
pub(in crate::journal) struct EventRetirementDisposition {
    pub(in crate::journal) lineage_id: i64,
    pub(in crate::journal) physical_generation: i64,
    pub(in crate::journal) reservation_id: i64,
    pub(in crate::journal) intent_id: i64,
    pub(in crate::journal) witness_request_id: i64,
    pub(in crate::journal) actual_request_id: i64,
    pub(in crate::journal) claim_generation: i64,
    pub(in crate::journal) status: EventRetirementStatus,
}

pub(super) async fn for_event(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    scale_set_id: i64,
    actual_request_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<Option<EventRetirementDisposition>, HostError> {
    if scale_set_id <= 0 || actual_request_id <= 0 || runner_id <= 0 || runner_name.is_empty() {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT c.lineage_id,c.generation,c.reservation_id,c.intent_id,c.witness_request_id,MAX(c.claim_generation),MAX(CASE WHEN retired.reservation_id IS NULL THEN 0 ELSE 1 END),auth.source_json,auth.source_digest FROM physical_attempt_post_jit_retirement_claims c JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id JOIN connection_runner_jits jit ON jit.intent_id=c.intent_id AND jit.connection_id=c.connection_id AND jit.scale_set_id=c.scale_set_id AND jit.witness_request_id=c.witness_request_id AND jit.runner_id=c.runner_id AND jit.runner_name=c.runner_name JOIN connection_request_authorizations witness ON witness.connection_id=c.connection_id AND witness.scale_set_id=c.scale_set_id AND witness.request_id=c.witness_request_id AND witness.binding_digest=c.binding_digest AND witness.source_digest=c.source_digest AND witness.acquire_response_digest=c.acquire_response_digest AND witness.acquire_state='acquired' JOIN connection_request_authorizations auth ON auth.connection_id=?1 AND auth.scale_set_id=?2 AND auth.request_id=?3 AND auth.binding_digest=c.binding_digest AND auth.acquire_state='acquired' LEFT JOIN physical_attempt_post_jit_retirements retired ON retired.lineage_id=c.lineage_id AND retired.generation=c.generation AND retired.reservation_id=c.reservation_id WHERE c.connection_id=?1 AND c.scale_set_id=?2 AND c.binding_digest=?4 AND c.runner_id=?5 AND c.runner_name=?6 AND a.connection_id=c.connection_id AND a.scale_set_id=c.scale_set_id GROUP BY c.lineage_id,c.generation,c.reservation_id,c.intent_id,c.witness_request_id,auth.source_json,auth.source_digest",
            (scope.connection_id().as_str(), scale_set_id, actual_request_id, scope.binding_digest(), runner_id, runner_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let disposition = parse_disposition(&row, actual_request_id)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Identity);
    }
    Ok(Some(disposition))
}

fn parse_disposition(
    row: &turso::Row,
    actual_request_id: i64,
) -> Result<EventRetirementDisposition, HostError> {
    let source_json: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
    let source_digest: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let (Some(source_json), Some(source_digest)) = (source_json, source_digest) else {
        return Err(HostError::Identity);
    };
    if super::identity::hex_digest(source_json.as_bytes()) != source_digest {
        return Err(HostError::Identity);
    }
    Ok(EventRetirementDisposition {
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        physical_generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
        intent_id: row.get(3).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(4).map_err(|_| HostError::Journal)?,
        actual_request_id,
        claim_generation: row.get(5).map_err(|_| HostError::Journal)?,
        status: if row.get::<i64>(6).map_err(|_| HostError::Journal)? == 1 {
            EventRetirementStatus::Retired
        } else {
            EventRetirementStatus::Claimed
        },
    })
}
