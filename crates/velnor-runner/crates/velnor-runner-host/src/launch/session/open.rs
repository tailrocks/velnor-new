use velnor_runner_github::QueueSession;

use super::{
    SessionDeleteOutcome, delete_removed_connection_session, map_journal,
    refresh_bound_connection_session,
};
use crate::journal::{ConnectionJournal, HeaderAllocation, SessionCreateState};
use crate::listen::{Link, OWNER_NAME, Secret, annotate};
use crate::scale_set::EnsureError;

/// Create one scoped session after storing its create intent.
pub(crate) async fn open_connection_session(
    link: &mut Link,
    set_id: i64,
    journal: &ConnectionJournal,
    requested_header_capacity: u32,
) -> Result<(QueueSession, i64, HeaderAllocation), EnsureError> {
    validate_connection_set(journal, set_id).await?;
    let records = journal.session_creates().await.map_err(map_journal)?;
    let active = records
        .iter()
        .filter(|record| {
            record.scale_set_id() == set_id
                && matches!(
                    record.state(),
                    SessionCreateState::Prepared
                        | SessionCreateState::Requested
                        | SessionCreateState::Uncertain
                        | SessionCreateState::Bound
                )
        })
        .collect::<Vec<_>>();
    if active.len() > 1 {
        return Err(EnsureError::Uncertain);
    }
    if let Some(record) = active.first()
        && let Some(bound) = ensure_bound_session(
            link,
            journal,
            set_id,
            requested_header_capacity,
            record.state(),
            record.session_id(),
            record.intent_id(),
        )
        .await?
    {
        return Ok(bound);
    }
    create_connection_session(
        link,
        journal,
        set_id,
        requested_header_capacity,
        active
            .first()
            .map(|record| (record.state(), record.local_nonce().to_owned())),
    )
    .await
}

async fn validate_connection_set(
    journal: &ConnectionJournal,
    set_id: i64,
) -> Result<(), EnsureError> {
    let binding = journal
        .source_authorization_binding()
        .await
        .map_err(map_journal)?;
    let registered = journal
        .scale_set()
        .await
        .map_err(map_journal)?
        .ok_or(EnsureError::AdmissionClosed)?;
    if binding.scale_set_id() != set_id || registered.id != set_id {
        return Err(EnsureError::AdmissionClosed);
    }
    Ok(())
}

async fn ensure_bound_session(
    link: &mut Link,
    journal: &ConnectionJournal,
    set_id: i64,
    requested_header_capacity: u32,
    state: SessionCreateState,
    recorded_session_id: Option<&str>,
    recorded_intent_id: Option<i64>,
) -> Result<Option<(QueueSession, i64, HeaderAllocation)>, EnsureError> {
    if state != SessionCreateState::Bound {
        return Ok(None);
    }
    let session_id = recorded_session_id.ok_or(EnsureError::AdmissionClosed)?;
    let intent_id = recorded_intent_id.ok_or(EnsureError::AdmissionClosed)?;
    let allocation = journal
        .session_header_allocation(intent_id)
        .await
        .map_err(map_journal)?;
    if allocation.connection_id() != journal.connection_id()
        || allocation.scale_set_id() != set_id
        || allocation.session_id() != session_id
    {
        return Err(EnsureError::AdmissionClosed);
    }
    let mut session =
        refresh_bound_connection_session(link, journal, intent_id, set_id, session_id).await?;
    if allocation.target() == requested_header_capacity {
        return Ok(Some((session, intent_id, allocation)));
    }
    remove_drained_bound_session(link, journal, set_id, &mut session, intent_id, &allocation)
        .await?;
    Ok(None)
}

async fn remove_drained_bound_session(
    link: &mut Link,
    journal: &ConnectionJournal,
    set_id: i64,
    session: &mut QueueSession,
    intent_id: i64,
    allocation: &HeaderAllocation,
) -> Result<(), EnsureError> {
    let polled = crate::launch::poll_scoped_connection_once(
        link, set_id, session, journal, intent_id, allocation, 0,
    )
    .await?;
    if !polled.session_delete_candidate
        || !journal
            .session_work_drained(intent_id)
            .await
            .map_err(map_journal)?
    {
        return Err(EnsureError::AdmissionClosed);
    }
    if delete_removed_connection_session(link, session, journal, intent_id, allocation).await
        != SessionDeleteOutcome::Deleted
    {
        return Err(EnsureError::Uncertain);
    }
    Ok(())
}

async fn create_connection_session(
    link: &mut Link,
    journal: &ConnectionJournal,
    set_id: i64,
    requested_header_capacity: u32,
    active: Option<(SessionCreateState, String)>,
) -> Result<(QueueSession, i64, HeaderAllocation), EnsureError> {
    let nonce = match active {
        Some((SessionCreateState::Prepared, nonce)) => nonce,
        Some((SessionCreateState::Requested | SessionCreateState::Uncertain, _)) => {
            // A lost create response or a bound session needs exact recovery
            // before another session can be opened.
            return Err(EnsureError::Uncertain);
        }
        Some(_) => return Err(EnsureError::AdmissionClosed),
        None => uuid::Uuid::new_v4().simple().to_string(),
    };
    let prepared = journal
        .prepare_session_create(&nonce)
        .await
        .map_err(map_journal)?;
    if prepared.state() != SessionCreateState::Prepared {
        return Err(EnsureError::Uncertain);
    }
    let claim = journal
        .claim_session_create(&nonce)
        .await
        .map_err(map_journal)?
        .ok_or(EnsureError::Uncertain)?;
    let admin_token = Secret::new(link.token());
    let session = match velnor_runner_github::create_session(
        link.transport(),
        set_id,
        OWNER_NAME,
        admin_token.expose(),
    ) {
        Ok(session) => session,
        Err(error) => {
            journal
                .mark_session_create_uncertain(&claim)
                .await
                .map_err(map_journal)?;
            return Err(annotate(error, "create-session"));
        }
    };
    let bound = journal
        .record_created_session(&claim, &session.session_id, requested_header_capacity)
        .await
        .map_err(map_journal)?;
    Ok((session, bound.intent_id(), bound.allocation().clone()))
}
