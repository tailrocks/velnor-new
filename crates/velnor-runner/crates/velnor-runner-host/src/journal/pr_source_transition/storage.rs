use serde_json::from_str;
use serde_json::to_string;

use crate::error::HostError;

use super::model::{TransitionEvent, TransitionLedger, TransitionPlan};
use crate::journal::Journal;

const MAX_PLAN_BYTES: usize = 8 * 1024;
const MAX_EVENT_BYTES: usize = 32 * 1024;
const MAX_EVENTS: usize = 16;

pub(super) async fn begin(
    journal: &Journal,
    plan: TransitionPlan,
) -> Result<TransitionLedger, HostError> {
    let digest = plan.digest().map_err(|_| HostError::Journal)?;
    let transition_id = plan.transition_id().map_err(|_| HostError::Journal)?;
    let json = to_string(&plan).map_err(|_| HostError::Journal)?;
    if json.len() > MAX_PLAN_BYTES {
        return Err(HostError::Journal);
    }
    let connection = journal.connection().await?;
    begin_immediate(&connection).await?;
    let result = async {
        let existing = load_by_id(&connection, &transition_id).await;
        match existing {
            Ok(ledger) if ledger.plan() == &plan => Ok(ledger),
            Ok(_) => Err(HostError::Journal),
            Err(HostError::Journal) => {
                connection
                    .execute(
                        "INSERT INTO pr_source_transitions (transition_id, plan_digest, plan_json, created_at) VALUES (?1, ?2, ?3, unixepoch())",
                        (transition_id.as_str(), digest.as_str(), json.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                Ok(TransitionLedger::new(plan).map_err(|_| HostError::Journal)?)
            }
            Err(error) => Err(error),
        }
    }
    .await;
    finish_transaction(&connection, result).await
}

pub(super) async fn load(
    journal: &Journal,
    transition_id: &str,
) -> Result<TransitionLedger, HostError> {
    if !valid_id(transition_id) {
        return Err(HostError::Journal);
    }
    let connection = journal.connection().await?;
    connection
        .execute("BEGIN", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = load_by_id(&connection, transition_id).await;
    finish_transaction(&connection, result).await
}

pub(super) async fn append(
    journal: &Journal,
    transition_id: &str,
    expected_plan_digest: &str,
    expected_event_count: usize,
    event: TransitionEvent,
) -> Result<TransitionLedger, HostError> {
    if !valid_id(transition_id)
        || !valid_id(expected_plan_digest)
        || expected_event_count >= MAX_EVENTS
    {
        return Err(HostError::Journal);
    }
    let event_json = to_string(&event).map_err(|_| HostError::Journal)?;
    if event_json.len() > MAX_EVENT_BYTES {
        return Err(HostError::Journal);
    }
    let connection = journal.connection().await?;
    begin_immediate(&connection).await?;
    let result = async {
        let mut ledger = load_by_id(&connection, transition_id).await?;
        if ledger.events().len() != expected_event_count
            || ledger.plan().digest().map_err(|_| HostError::Journal)?
                != expected_plan_digest
        {
            return Err(HostError::Journal);
        }
        ledger
            .append_validated(event)
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "INSERT INTO pr_source_transition_events (transition_id, sequence, event_json) VALUES (?1, ?2, ?3)",
                (transition_id, i64::try_from(expected_event_count).map_err(|_| HostError::Journal)?, event_json.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(ledger)
    }
    .await;
    finish_transaction(&connection, result).await
}

pub(crate) async fn validate_persisted_rows(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT transition_id FROM pr_source_transitions ORDER BY transition_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: String = row.get(0).map_err(|_| HostError::Journal)?;
        if !valid_id(&id) {
            return Err(HostError::Journal);
        }
        ids.push(id);
    }
    drop(rows);
    for id in ids {
        load_by_id(connection, &id).await?;
    }
    Ok(())
}

async fn load_by_id(
    connection: &turso::Connection,
    transition_id: &str,
) -> Result<TransitionLedger, HostError> {
    let mut rows = connection
        .query(
            "SELECT plan_digest, plan_json FROM pr_source_transitions WHERE transition_id = ?1",
            (transition_id,),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let stored_digest: String = row.get(0).map_err(|_| HostError::Journal)?;
    let plan_json: String = row.get(1).map_err(|_| HostError::Journal)?;
    if plan_json.len() > MAX_PLAN_BYTES {
        return Err(HostError::Journal);
    }
    drop(rows);
    let plan: TransitionPlan = from_str(&plan_json).map_err(|_| HostError::Journal)?;
    if plan.digest().map_err(|_| HostError::Journal)? != stored_digest
        || plan.transition_id().map_err(|_| HostError::Journal)? != transition_id
    {
        return Err(HostError::Journal);
    }

    let mut rows = connection
        .query(
            "SELECT sequence, event_json FROM pr_source_transition_events WHERE transition_id = ?1 ORDER BY sequence",
            (transition_id,),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut events = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if events.len() >= MAX_EVENTS {
            return Err(HostError::Journal);
        }
        let sequence: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let event_json: String = row.get(1).map_err(|_| HostError::Journal)?;
        if usize::try_from(sequence).ok() != Some(events.len())
            || event_json.len() > MAX_EVENT_BYTES
        {
            return Err(HostError::Journal);
        }
        events.push(from_str(&event_json).map_err(|_| HostError::Journal)?);
    }
    TransitionLedger::from_events(plan, events).map_err(|_| HostError::Journal)
}

async fn begin_immediate(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn finish_transaction<T>(
    connection: &turso::Connection,
    result: Result<T, HostError>,
) -> Result<T, HostError> {
    let end = if result.is_ok() {
        connection.execute("COMMIT", ()).await
    } else {
        connection.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

fn valid_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
