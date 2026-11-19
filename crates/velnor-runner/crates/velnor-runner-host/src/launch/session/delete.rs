// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{QueueSession, SessionClose, Transport, delete_session};

use crate::journal::{ConnectionJournal, HeaderAllocation, ScaleSetDeleteGate};
use crate::listen::{Link, Secret};

use super::{DeletedSession204, SessionDeleteOutcome};

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

    let closed = match delete_session(
        transport,
        allocation.scale_set_id(),
        allocation.session_id(),
        admin_token,
    ) {
        Ok(SessionClose::Closed) => true,
        Ok(SessionClose::Gone) => created_set_deleted_by_us(scoped).await,
        Err(_) => false,
    };
    if !closed {
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

/// A 404 on the session is "already gone" only when the journal proves this
/// controller deleted the set that held it: deleting the set takes its
/// sessions with it, and `disconnect --wait` deletes a created set before the
/// connection leaves `host.toml`, which is when the session delete runs.
async fn created_set_deleted_by_us(scoped: &ConnectionJournal) -> bool {
    matches!(
        scoped.scale_set_delete_gate().await,
        Ok(ScaleSetDeleteGate::Deleted)
    )
}

#[cfg(test)]
#[path = "delete/tests.rs"]
mod tests;
