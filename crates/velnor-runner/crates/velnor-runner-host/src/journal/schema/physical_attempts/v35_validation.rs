use super::{REVISION_TABLES, normalize_sql};
use crate::error::HostError;

pub(super) async fn validate_rows_v35(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT (SELECT COUNT(*) FROM physical_attempt_current AS current LEFT JOIN physical_attempts AS attempt ON attempt.lineage_id=current.lineage_id AND attempt.generation=current.generation AND attempt.reservation_id=current.reservation_id AND attempt.intent_id=current.intent_id LEFT JOIN capacity_reservations AS reservation ON reservation.id=current.reservation_id LEFT JOIN connection_intents AS intent ON intent.id=current.intent_id WHERE attempt.lineage_id IS NULL OR reservation.state NOT IN ('reserved','uncertain','recovering') OR intent.state NOT IN ('pending','uncertain','done') OR EXISTS(SELECT 1 FROM physical_attempt_terminal_proofs proof WHERE proof.lineage_id=current.lineage_id AND proof.generation=current.generation AND proof.reservation_id=current.reservation_id)) + (SELECT COUNT(*) FROM (SELECT lineage_id,generation,reservation_id FROM physical_attempt_terminal_proofs GROUP BY lineage_id,generation,reservation_id HAVING COUNT(*) != 1)) + (SELECT COUNT(*) FROM physical_attempt_terminal_proofs proof JOIN physical_attempt_current current ON current.lineage_id=proof.lineage_id AND current.generation=proof.generation AND current.reservation_id=proof.reservation_id)",
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

pub(super) fn terminal_proof_guard(guard: &str) -> String {
    guard.replace(
        "physical_attempt_outcomes AS outcome",
        "physical_attempt_terminal_proofs AS outcome",
    )
}

pub(super) async fn validate_revision_triggers(conn: &turso::Connection) -> Result<(), HostError> {
    for table in REVISION_TABLES {
        for (suffix, action) in [
            ("insert", "after insert"),
            ("update", "after update"),
            ("delete", "after delete"),
        ] {
            let name = format!("{table}_revision_{suffix}");
            let expected = normalize_sql(&format!(
                "CREATE TRIGGER {name} {action} ON {table} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
            ));
            let mut rows = conn
                .query(
                    "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                    (name.as_str(),),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let actual: String = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
                .get(0)
                .map_err(|_| HostError::Journal)?;
            if normalize_sql(&actual) != expected
                || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}
