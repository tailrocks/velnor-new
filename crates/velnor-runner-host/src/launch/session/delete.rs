// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{QueueSession, SessionClose, Transport, delete_session};

use crate::journal::{ConnectionJournal, HeaderAllocation};
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
        Ok(SessionClose::Gone) => {
            created_set_deleted_by_us(scoped, allocation.scale_set_id()).await
        }
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

pub(crate) async fn retire_session_of_deleted_set(
    link: &mut Link,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
) -> Option<SessionDeleteOutcome> {
    let admin_token = Secret::new(link.token());
    retire_session_of_deleted_set_with(
        link.transport(),
        admin_token.expose(),
        session,
        scoped,
        session_intent_id,
    )
    .await
}

pub(super) async fn retire_session_of_deleted_set_with<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
) -> Option<SessionDeleteOutcome> {
    let allocation = scoped
        .session_header_allocation(session_intent_id)
        .await
        .ok()?;
    if !created_set_deleted_by_us(scoped, allocation.scale_set_id()).await {
        return None;
    }
    if scoped
        .settle_header_of_deleted_set(session_intent_id, &allocation)
        .await
        .is_err()
    {
        return Some(SessionDeleteOutcome::NotReady);
    }
    Some(
        delete_removed_connection_session_with(
            transport,
            admin_token,
            session,
            scoped,
            session_intent_id,
            &allocation,
        )
        .await,
    )
}

async fn created_set_deleted_by_us(scoped: &ConnectionJournal, scale_set_id: i64) -> bool {
    scoped
        .scale_set_recorded_deleted(scale_set_id)
        .await
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "delete/tests.rs"]
mod tests;
