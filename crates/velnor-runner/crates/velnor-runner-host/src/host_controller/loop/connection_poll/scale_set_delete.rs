// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
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
