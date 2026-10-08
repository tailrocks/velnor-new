//! Durable session row readers and atomic reservation helpers.

use crate::error::HostError;
use crate::journal::one_row;

use super::super::{KIND, ScaleSetSessionClosePermit, ScaleSetSessionIdentity};
use super::{ScaleSetSessionClaim, ScaleSetSessionCloseClaim};

pub(super) async fn claim_close(
    conn: &turso::Connection,
    identity: &ScaleSetSessionIdentity,
) -> Result<ScaleSetSessionCloseClaim, HostError> {
    let Some(active) = active_session(conn).await? else {
        return closed_session_result(conn, identity).await;
    };
    if active.intent_state != "pending" || active.effect_state != "may_have_effect" {
        return Err(HostError::Journal);
    }
    if active.subject != identity.subject
        || !identity.legacy_target_matches(active.target_repository_full_name.as_deref())
    {
        return Ok(ScaleSetSessionCloseClaim::Held);
    }
    if active.session_state != "open" || active.close_attempted {
        return Ok(ScaleSetSessionCloseClaim::Held);
    }
    let session_id = active.session_id.ok_or(HostError::Journal)?;
    one_row(
        conn.execute(
            "UPDATE scale_set_sessions SET close_attempted = 1 WHERE intent_id = ?1 AND state = 'open' AND close_attempted = 0 AND session_id = ?2",
            (active.intent_id, session_id.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?,
    )?;
    Ok(ScaleSetSessionCloseClaim::Claimed(Box::new(
        ScaleSetSessionClosePermit {
            intent_id: active.intent_id,
            identity: identity.clone(),
            session_id,
            delete_dispatch_started: false,
        },
    )))
}

async fn active_session(conn: &turso::Connection) -> Result<Option<ActiveSession>, HostError> {
    let mut rows = conn
        .query(
            "SELECT s.intent_id, s.session_id, s.state, s.target_repository_full_name, s.close_attempted, i.subject, i.state, i.effect_state FROM scale_set_sessions AS s JOIN intents AS i ON i.id = s.intent_id WHERE i.kind = ?1 AND s.state IN ('creating', 'open') ORDER BY s.intent_id LIMIT 2",
            [KIND],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let session = ActiveSession {
        intent_id: row.get(0).map_err(|_| HostError::Journal)?,
        session_id: row.get(1).map_err(|_| HostError::Journal)?,
        session_state: row.get(2).map_err(|_| HostError::Journal)?,
        target_repository_full_name: row.get(3).map_err(|_| HostError::Journal)?,
        close_attempted: parse_bool(row.get(4).map_err(|_| HostError::Journal)?)?,
        subject: row.get(5).map_err(|_| HostError::Journal)?,
        intent_state: row.get(6).map_err(|_| HostError::Journal)?,
        effect_state: row.get(7).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(session))
}

async fn closed_session_result(
    conn: &turso::Connection,
    identity: &ScaleSetSessionIdentity,
) -> Result<ScaleSetSessionCloseClaim, HostError> {
    let mut rows = conn
        .query(
            "SELECT s.state, s.session_id, s.target_repository_full_name, s.close_attempted, i.state, i.effect_state FROM scale_set_sessions AS s JOIN intents AS i ON i.id = s.intent_id WHERE i.kind = ?1 AND i.subject = ?2 ORDER BY s.intent_id DESC LIMIT 1",
            (KIND, identity.subject.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(ScaleSetSessionCloseClaim::Missing);
    };
    let state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let session_id: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let target_full_name: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let close_attempted = parse_bool(row.get(3).map_err(|_| HostError::Journal)?)?;
    let intent_state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let effect_state: String = row.get(5).map_err(|_| HostError::Journal)?;
    if !identity.legacy_target_matches(target_full_name.as_deref()) {
        return Ok(ScaleSetSessionCloseClaim::Missing);
    }
    if state == "closed"
        && session_id.is_some()
        && close_attempted
        && intent_state == "done"
        && effect_state == "may_have_effect"
    {
        return Ok(ScaleSetSessionCloseClaim::Closed);
    }
    if state == "closed"
        && session_id.is_none()
        && intent_state == "failed"
        && effect_state == "definite_no_effect"
    {
        return Ok(ScaleSetSessionCloseClaim::NoSession);
    }
    Ok(ScaleSetSessionCloseClaim::Held)
}

struct ActiveSession {
    intent_id: i64,
    session_id: Option<String>,
    session_state: String,
    target_repository_full_name: Option<String>,
    close_attempted: bool,
    subject: String,
    intent_state: String,
    effect_state: String,
}

pub(super) async fn reserve_session(
    conn: &turso::Connection,
    identity: &ScaleSetSessionIdentity,
) -> Result<ScaleSetSessionClaim, HostError> {
    let mut rows = conn
        .query(
            "SELECT s.intent_id, i.state, i.effect_state FROM scale_set_sessions AS s JOIN intents AS i ON i.id = s.intent_id WHERE i.kind = ?1 AND s.state IN ('creating', 'open') ORDER BY s.intent_id LIMIT 2",
            [KIND],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let first = rows.next().await.map_err(|_| HostError::Journal)?;
    let active = if let Some(row) = first {
        let id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let state = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let effect = row.get::<String>(2).map_err(|_| HostError::Journal)?;
        if state != "pending" || effect != "may_have_effect" {
            return Err(HostError::Journal);
        }
        Some(id)
    } else {
        None
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    drop(rows);
    if let Some(id) = active {
        return Ok(ScaleSetSessionClaim::Existing(id));
    }
    let mut rows = conn
        .query("SELECT draining FROM controller_state WHERE id = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let draining = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    match draining {
        1 => return Ok(ScaleSetSessionClaim::Draining),
        0 => {}
        _ => return Err(HostError::Journal),
    }
    conn.execute(
        "INSERT INTO intents (kind, subject, state, effect_state) VALUES (?1, ?2, 'pending', 'may_have_effect')",
        (KIND, identity.subject.as_str()),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO scale_set_sessions (intent_id, session_id, state, target_repository_full_name, close_attempted) VALUES (?1, NULL, 'creating', ?2, 0)",
        (id, identity.target_repository_full_name.as_str()),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(ScaleSetSessionClaim::Reserved(id))
}

pub(super) async fn session_row(
    conn: &turso::Connection,
    intent_id: i64,
) -> Result<(String, Option<String>, String, String, bool), HostError> {
    let mut rows = conn
        .query(
            "SELECT s.state, s.session_id, i.state, i.effect_state, s.close_attempted FROM scale_set_sessions AS s JOIN intents AS i ON i.id = s.intent_id WHERE s.intent_id = ?1 AND i.kind = ?2",
            (intent_id, KIND),
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
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
        parse_bool(row.get(4).map_err(|_| HostError::Journal)?)?,
    ))
}

pub(super) async fn finish_transaction<T>(
    conn: &turso::Connection,
    result: Result<T, HostError>,
) -> Result<T, HostError> {
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

fn parse_bool(value: i64) -> Result<bool, HostError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}
