use velnor_runner_github::{QueueSession, Transport, refresh_session};

use crate::journal::ConnectionJournal;
use crate::listen::{Link, Secret, annotate, validate_queue_url};
use crate::scale_set::EnsureError;

use super::{ReturnedSessionRefreshTransportError, VerifiedSessionRefresh, map_journal};

/// Refresh one bound session after a durable generation claim.
///
/// The returned queue credentials stay in memory and are usable only after the
/// exact ID, queue route, and generation receipt all pass.
pub(crate) async fn refresh_bound_connection_session(
    link: &mut Link,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    scale_set_id: i64,
    expected_session_id: &str,
) -> Result<QueueSession, EnsureError> {
    let admin_token = Secret::new(link.token());
    let admin_base = link.base().to_owned();
    refresh_bound_connection_session_with(
        link.transport(),
        admin_token.expose(),
        &admin_base,
        scoped,
        session_intent_id,
        scale_set_id,
        expected_session_id,
    )
    .await
}

pub(super) async fn refresh_bound_connection_session_with<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    admin_base: &str,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    scale_set_id: i64,
    expected_session_id: &str,
) -> Result<QueueSession, EnsureError> {
    let allocation = scoped
        .session_header_allocation(session_intent_id)
        .await
        .map_err(map_journal)?;
    if session_intent_id <= 0
        || scale_set_id <= 0
        || expected_session_id.is_empty()
        || allocation.connection_id() != scoped.connection_id()
        || allocation.scale_set_id() != scale_set_id
        || allocation.session_id() != expected_session_id
    {
        return Err(EnsureError::AdmissionClosed);
    }

    let claim = scoped
        .claim_session_refresh(session_intent_id, expected_session_id)
        .await
        .map_err(map_journal)?
        .ok_or(EnsureError::Uncertain)?;
    if claim.session_intent_id() != session_intent_id
        || claim.scale_set_id() != scale_set_id
        || claim.session_id() != expected_session_id
    {
        scoped
            .mark_session_refresh_uncertain(claim)
            .await
            .map_err(map_journal)?;
        return Err(EnsureError::AdmissionClosed);
    }

    let returned = refresh_session(transport, scale_set_id, expected_session_id, admin_token);
    let transport_proof =
        ReturnedSessionRefreshTransportError::from_returned_result(&claim, &returned);
    let session = match returned {
        Ok(session) => session,
        Err(error) => {
            if let Some(proof) = transport_proof {
                scoped
                    .record_session_refresh_transport_error(claim, proof)
                    .await
                    .map_err(map_journal)?;
            } else {
                scoped
                    .mark_session_refresh_uncertain(claim)
                    .await
                    .map_err(map_journal)?;
            }
            return Err(annotate(error, "refresh-session"));
        }
    };

    if session.session_id != expected_session_id {
        scoped
            .mark_session_refresh_uncertain(claim)
            .await
            .map_err(map_journal)?;
        return Err(EnsureError::AdmissionClosed);
    }
    if let Err(error) = validate_queue_url(admin_base, &session.message_queue_url) {
        scoped
            .mark_session_refresh_uncertain(claim)
            .await
            .map_err(map_journal)?;
        return Err(error);
    }
    let proof = match VerifiedSessionRefresh::after_response(&claim, &session) {
        Ok(proof) => proof,
        Err(error) => {
            scoped
                .mark_session_refresh_uncertain(claim)
                .await
                .map_err(map_journal)?;
            return Err(error);
        }
    };
    scoped
        .record_session_refreshed(claim, proof)
        .await
        .map_err(map_journal)?;
    crate::launch::trace::session(&session);
    Ok(session)
}
