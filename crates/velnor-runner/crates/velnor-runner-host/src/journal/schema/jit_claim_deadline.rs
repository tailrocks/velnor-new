// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const OBJECTS: &[(&str, &str, &str)] = &[
    (
        "table",
        "jit_dispatch_claim_times",
        "CREATE TABLE jit_dispatch_claim_times (intent_id INTEGER PRIMARY KEY REFERENCES connection_runner_jits(intent_id), commit_observed_at INTEGER CHECK(commit_observed_at>=0))",
    ),
    (
        "trigger",
        "jit_dispatch_claim_time",
        "CREATE TRIGGER jit_dispatch_claim_time AFTER UPDATE OF jit_state ON capacity_reservation_provisioning_state WHEN NEW.jit_state='requested' AND OLD.jit_state='not_requested' BEGIN INSERT INTO jit_dispatch_claim_times(intent_id,commit_observed_at) SELECT jit.intent_id,NULL FROM connection_runner_jits jit JOIN capacity_reservations reservation ON reservation.intent_id=jit.intent_id WHERE reservation.id=NEW.reservation_id AND jit.state='claimed'; END",
    ),
    (
        "trigger",
        "jit_dispatch_claim_time_guard",
        "CREATE TRIGGER jit_dispatch_claim_time_guard BEFORE INSERT ON jit_dispatch_claim_times WHEN NEW.commit_observed_at!=unixepoch()+1 OR NOT EXISTS(SELECT 1 FROM connection_runner_jits jit WHERE jit.intent_id=NEW.intent_id) BEGIN SELECT RAISE(ABORT,'The JIT commit observation must use the current clock.'); END",
    ),
    (
        "trigger",
        "jit_dispatch_claim_time_no_update",
        "CREATE TRIGGER jit_dispatch_claim_time_no_update BEFORE UPDATE ON jit_dispatch_claim_times WHEN OLD.commit_observed_at IS NOT NULL OR NEW.commit_observed_at IS NULL OR NEW.commit_observed_at!=unixepoch()+1 OR NEW.intent_id!=OLD.intent_id BEGIN SELECT RAISE(ABORT,'JIT dispatch claim times are immutable.'); END",
    ),
    (
        "trigger",
        "jit_dispatch_claim_time_no_delete",
        "CREATE TRIGGER jit_dispatch_claim_time_no_delete BEFORE DELETE ON jit_dispatch_claim_times BEGIN SELECT RAISE(ABORT,'JIT dispatch claim times are immutable.'); END",
    ),
    (
        "table",
        "jit_absent_after_deadline",
        "CREATE TABLE jit_absent_after_deadline (reservation_id INTEGER PRIMARY KEY, claim_generation INTEGER NOT NULL, evaluated_at INTEGER NOT NULL CHECK(evaluated_at>=0), FOREIGN KEY(reservation_id,claim_generation) REFERENCES physical_attempt_jit_conflict_recovery_claims(reservation_id,claim_generation))",
    ),
    (
        "trigger",
        "jit_absent_after_deadline_guard",
        "CREATE TRIGGER jit_absent_after_deadline_guard BEFORE INSERT ON jit_absent_after_deadline WHEN NOT EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_claims claim JOIN jit_dispatch_claim_times timing ON timing.intent_id=claim.intent_id JOIN physical_attempt_jit_interrupted_claims interrupted ON interrupted.reservation_id=claim.reservation_id WHERE claim.reservation_id=NEW.reservation_id AND claim.claim_generation=NEW.claim_generation AND NEW.evaluated_at>=timing.commit_observed_at+60 AND claim.lease_until>unixepoch()) BEGIN SELECT RAISE(ABORT,'The JIT dispatch deadline has not elapsed.'); END",
    ),
    (
        "trigger",
        "jit_absent_after_deadline_no_update",
        "CREATE TRIGGER jit_absent_after_deadline_no_update BEFORE UPDATE ON jit_absent_after_deadline BEGIN SELECT RAISE(ABORT,'JIT dispatch decisions are immutable.'); END",
    ),
    (
        "trigger",
        "jit_absent_after_deadline_no_delete",
        "CREATE TRIGGER jit_absent_after_deadline_no_delete BEFORE DELETE ON jit_absent_after_deadline BEGIN SELECT RAISE(ABORT,'JIT dispatch decisions are immutable.'); END",
    ),
];

const GUARD: &str = "CREATE TRIGGER jit_interrupted_remote_absence_guard BEFORE INSERT ON physical_attempt_jit_conflict_recovery_progress WHEN (NEW.phase = 'remote_initial_absent' OR (NEW.phase = 'remote_absent' AND NEW.runner_id IS NULL)) AND EXISTS (SELECT 1 FROM physical_attempt_jit_interrupted_claims interrupted WHERE interrupted.reservation_id = NEW.reservation_id) AND NOT EXISTS(SELECT 1 FROM jit_absent_after_deadline decision JOIN jit_dispatch_claim_times timing ON timing.intent_id=(SELECT intent_id FROM physical_attempt_jit_conflict_recovery_claims WHERE reservation_id=NEW.reservation_id AND claim_generation=NEW.claim_generation) WHERE decision.reservation_id=NEW.reservation_id AND decision.evaluated_at>=timing.commit_observed_at+60) BEGIN SELECT RAISE(ABORT, 'Interrupted JIT registration requires an elapsed dispatch deadline.'); END";

pub(super) async fn recover_times(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute(
        "UPDATE jit_dispatch_claim_times SET commit_observed_at=unixepoch()+1 WHERE commit_observed_at IS NULL",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn migrate(conn: &turso::Connection) -> Result<(), HostError> {
    super::jit_interrupted_claims::validate_initial_absence(conn).await?;
    for (_, _, sql) in OBJECTS {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute("INSERT INTO jit_dispatch_claim_times SELECT intent_id,unixepoch()+1 FROM connection_runner_jits", ()).await.map_err(|_| HostError::Journal)?;
    conn.execute("DROP TRIGGER jit_interrupted_remote_absence_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version=49", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for &(kind, name, expected) in OBJECTS.iter().chain(std::iter::once(&(
        "trigger",
        "jit_interrupted_remote_absence_guard",
        GUARD,
    ))) {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
                (kind, name),
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
        if super::sql_normalization::normalize(&actual)
            != super::sql_normalization::normalize(expected)
        {
            return Err(HostError::Journal);
        }
    }
    let mut rows = conn.query("SELECT EXISTS(SELECT 1 FROM jit_absent_after_deadline decision JOIN physical_attempt_jit_conflict_recovery_claims claim ON claim.reservation_id=decision.reservation_id AND claim.claim_generation=decision.claim_generation LEFT JOIN jit_dispatch_claim_times timing ON timing.intent_id=claim.intent_id WHERE timing.intent_id IS NULL OR decision.evaluated_at<timing.commit_observed_at+60) OR EXISTS(SELECT 1 FROM physical_attempt_jit_conflict_recovery_progress progress JOIN physical_attempt_jit_interrupted_claims interrupted ON interrupted.reservation_id=progress.reservation_id WHERE (progress.phase='remote_initial_absent' OR (progress.phase='remote_absent' AND progress.runner_id IS NULL)) AND NOT EXISTS(SELECT 1 FROM jit_absent_after_deadline decision WHERE decision.reservation_id=progress.reservation_id))", ()).await.map_err(|_| HostError::Journal)?;
    if rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
        != 0
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
