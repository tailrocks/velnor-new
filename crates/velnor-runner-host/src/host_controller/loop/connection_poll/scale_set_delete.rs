// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
    launch::{SessionDeleteOutcome, retire_session_of_deleted_set},
    listen::Secret,
    scale_set::delete::{ScaleSetDeleteOutcome, run_created_delete},
};

use super::{ConnectionPollContext, PreparedConnection};

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
) -> Result<Option<bool>, HostError> {
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Ok(None);
    };
    let Some(outcome) = retire_session_of_deleted_set(
        &mut credentials.link,
        &credentials.session,
        &prepared.scope,
        prepared.session_intent,
    )
    .await
    else {
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
