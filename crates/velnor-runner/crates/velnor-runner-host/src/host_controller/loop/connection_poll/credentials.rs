// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::RepositoryConnection, error::HostError, launch::open_connection_session,
    launch_blocking::ListenFault,
};

use super::{
    super::{
        setup::{ConnectionLaunchOptions, PrepareConnectionRequest, restore_authority},
        state::{PreparedConnection, SessionCredentials},
    },
    ConnectionPollContext,
};

pub(super) async fn restore_missing<C, E>(
    slot: &mut Option<C>,
    restore: impl AsyncFnOnce() -> Result<C, E>,
) -> Result<(), E> {
    if slot.is_none() {
        *slot = Some(restore().await?);
    }
    Ok(())
}

pub(super) async fn restore_credentials(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
) -> Result<(), ListenFault> {
    let binding: &RepositoryConnection = &prepared.binding;
    let scope = &prepared.scope;
    let set_id = prepared.set_id;
    let session_intent = prepared.session_intent;
    restore_missing(&mut prepared.credentials, async || {
        let request = PrepareConnectionRequest {
            state: context.state,
            journal: context.journal,
            binding,
            config: context.config,
            selected_engine: context.selected_engine,
            worker_images: context.worker_images,
            docker: context.docker,
            options: ConnectionLaunchOptions {
                image_authority: context.image_authority,
                requested_header_capacity: 0,
            },
        };
        let (mut link, github_pat, restored_set_id) = restore_authority(&request).await?;
        let target = scope
            .session_header_allocation(session_intent)
            .await?
            .target();
        let (session, intent, _allocation) =
            open_connection_session(&mut link, set_id, scope, target).await?;
        if restored_set_id != set_id || intent != session_intent {
            return Err(HostError::Ownership.into());
        }
        Ok(SessionCredentials {
            link,
            session,
            github_pat,
        })
    })
    .await
}

#[cfg(test)]
#[path = "credentials/tests.rs"]
mod tests;
