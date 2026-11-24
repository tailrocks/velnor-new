// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use crate::{
    config::RepositoryConnection,
    error::HostError,
    launch::{SessionDeleteOutcome, retire_session_of_deleted_set},
    listen::Secret,
    scale_set::delete::{ScaleSetDeleteOutcome, run_created_delete},
};

use super::{
    ConnectionPollContext, PreparedConnection,
    credentials::{RESTORE_ATTEMPT_DEADLINE, RESTORE_RETRY, restore_link_before_deadline},
};

pub(super) async fn service(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
) -> Result<(), HostError> {
    let connection_id = prepared.binding.connection_id.as_str().to_owned();
    let Some(nonce) = context
        .control_state
        .pending_scale_set_delete(&connection_id)?
    else {
        return Ok(());
    };
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Ok(());
    };
    let admin_token = Secret::new(credentials.link.token());
    let outcome = match run_created_delete(
        credentials.link.transport(),
        admin_token.expose(),
        &prepared.scope,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!(
                "host controller scale set delete held connection={connection_id} reason={error}"
            );
            ScaleSetDeleteOutcome::Uncertain
        }
    };
    context
        .control_state
        .finish_scale_set_delete(&connection_id, &nonce, outcome)
}

pub(super) async fn retire_deleted_set_session(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
) -> Result<Option<bool>, HostError> {
    let outcome = match prepared.credentials.as_mut() {
        Some(credentials) => {
            retire_session_of_deleted_set(
                &mut credentials.link,
                &credentials.session.session_id,
                &prepared.scope,
                prepared.session_intent,
            )
            .await
        }
        None => retire_without_credentials(prepared, context, pending_retirements).await,
    };
    let Some(outcome) = outcome else {
        return Ok(None);
    };
    context
        .control_state
        .invalidate_session_poll(prepared.binding.connection_id.as_str())?;
    if outcome == SessionDeleteOutcome::NotReady {
        return Ok(Some(false));
    }
    prepared.credentials.take();
    if let Some(completion) = prepared.completion.take()
        && let Err(error) = completion.shutdown().await
    {
        eprintln!(
            "host controller completion shutdown held connection={} reason={error}",
            prepared.binding.connection_id
        );
    }
    Ok(Some(outcome == SessionDeleteOutcome::Deleted))
}

async fn retire_without_credentials(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
) -> Option<SessionDeleteOutcome> {
    let configured = context
        .config
        .connection(&prepared.binding.connection_id)
        .is_some();
    if !configured
        && !pending_retirements
            .iter()
            .any(|binding| prepared.is_for(binding))
    {
        return None;
    }
    if !prepared
        .scope
        .scale_set_recorded_deleted(prepared.set_id)
        .await
        .ok()?
    {
        return None;
    }
    let session_id = prepared
        .scope
        .session_header_allocation(prepared.session_intent)
        .await
        .ok()?
        .session_id()
        .to_owned();
    if prepared
        .restore_retry_at
        .is_some_and(|at| tokio::time::Instant::now().into_std() < at)
    {
        return Some(SessionDeleteOutcome::NotReady);
    }
    let deadline = tokio::time::Instant::now() + RESTORE_ATTEMPT_DEADLINE;
    let restored = restore_link_before_deadline(prepared, context, deadline).await;
    let Ok((mut link, _pat, restored_set_id)) = restored else {
        prepared.restore_retry_at = Some(Instant::now() + RESTORE_RETRY);
        return Some(SessionDeleteOutcome::NotReady);
    };
    if restored_set_id != prepared.set_id {
        prepared.restore_retry_at = Some(Instant::now() + RESTORE_RETRY);
        return Some(SessionDeleteOutcome::NotReady);
    }
    prepared.restore_retry_at = None;
    retire_session_of_deleted_set(
        &mut link,
        &session_id,
        &prepared.scope,
        prepared.session_intent,
    )
    .await
}
