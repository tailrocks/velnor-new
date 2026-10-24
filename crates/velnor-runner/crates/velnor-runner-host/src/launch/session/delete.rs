use velnor_runner_github::{QueueSession, Transport, delete_session};

use crate::journal::{ConnectionJournal, HeaderAllocation};
use crate::listen::{Link, Secret};

use super::{DeletedSession204, SessionDeleteOutcome};

/// Delete only the exact bound session after its zero-header poll and local drain.
///
/// Any effect error or proof-write failure leaves the durable attempt in-flight or uncertain.
pub(crate) async fn delete_removed_connection_session(
    link: &mut Link,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> SessionDeleteOutcome {
    let admin_token = Secret::new(link.token());
    delete_removed_connection_session_with(
        link.transport(),
        admin_token.expose(),
        session,
        scoped,
        session_intent_id,
        allocation,
    )
    .await
}

pub(super) async fn delete_removed_connection_session_with<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> SessionDeleteOutcome {
    if allocation.connection_id() != scoped.connection_id()
        || allocation.session_id() != session.session_id
    {
        return SessionDeleteOutcome::NotReady;
    }
    let Ok(attempt) = scoped
        .begin_session_delete(session_intent_id, allocation)
        .await
    else {
        return SessionDeleteOutcome::NotReady;
    };

    if delete_session(
        transport,
        allocation.scale_set_id(),
        allocation.session_id(),
        admin_token,
    )
    .is_err()
    {
        let _ignored = scoped
            .mark_session_delete_uncertain(session_intent_id, attempt)
            .await;
        return SessionDeleteOutcome::Held;
    }

    let Ok(proof) = DeletedSession204::after_response(
        attempt,
        allocation,
        scoped,
        allocation.scale_set_id(),
        &session.session_id,
    ) else {
        return SessionDeleteOutcome::Held;
    };
    match scoped
        .retire_session_after_delete(session_intent_id, proof)
        .await
    {
        Ok(()) => SessionDeleteOutcome::Deleted,
        Err(_) => SessionDeleteOutcome::Held,
    }
}
