// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    fmt::Display,
    time::{Duration, Instant},
};

use crate::{
    config::RepositoryConnection, error::HostError, journal::ConnectionJournal,
    launch::open_connection_session, launch_blocking::ListenFault, scale_set::EnsureError,
};

use super::{
    super::{
        setup::{ConnectionLaunchOptions, PrepareConnectionRequest, restore_authority},
        state::{PreparedConnection, SessionCredentials},
    },
    ConnectionPollContext,
};

const RESTORE_RETRY: Duration = Duration::from_secs(30);
const RESTORE_ATTEMPT_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestoreMode {
    Polling,
    RetirementOnly,
    Refused,
}

impl RestoreMode {
    pub(super) const fn of(configured: bool, pending_retirement: bool) -> Self {
        if configured {
            Self::Polling
        } else if pending_retirement {
            Self::RetirementOnly
        } else {
            Self::Refused
        }
    }
}

async fn restore_missing<C, E>(
    slot: &mut Option<C>,
    restore: impl AsyncFnOnce() -> Result<C, E>,
) -> Result<(), E> {
    if slot.is_none() {
        *slot = Some(restore().await?);
    }
    Ok(())
}

pub(super) fn release_suspect_credentials<C>(slot: &mut Option<C>, error: &HostError) {
    let suspect = [
        EnsureError::Forbidden,
        EnsureError::Rejected,
        EnsureError::Conflict,
        EnsureError::Unauthorized,
    ]
    .iter()
    .any(|cause| HostError::poll(cause) == *error);
    if suspect {
        slot.take();
    }
}

pub(super) async fn guarded_restore<C, E: Display>(
    connection_id: &str,
    mode: RestoreMode,
    clock: impl Fn() -> Instant,
    attempt_deadline: Duration,
    retry_at: &mut Option<Instant>,
    slot: &mut Option<C>,
    restore: impl AsyncFnOnce() -> Result<C, E>,
) -> bool {
    if mode == RestoreMode::Refused || slot.is_some() {
        return true;
    }
    if retry_at.is_some_and(|at| clock() < at) {
        return false;
    }
    let outcome = match tokio::time::timeout(attempt_deadline, restore_missing(slot, restore)).await
    {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(_) => Some("restore attempt deadline exceeded".to_owned()),
    };
    match outcome {
        None => {
            *retry_at = None;
            true
        }
        Some(reason) => {
            eprintln!(
                "host controller credential restore held connection={connection_id} mode={mode:?} reason={reason}"
            );
            *retry_at = Some(clock() + RESTORE_RETRY);
            false
        }
    }
}

pub(super) async fn restore_bound_session<C, E: From<HostError>>(
    scope: &ConnectionJournal,
    set_id: i64,
    restore: impl AsyncFnOnce() -> Result<C, E>,
) -> Result<C, E> {
    if !scope.has_bound_session(set_id).await? {
        return Err(HostError::Ownership.into());
    }
    restore().await
}

pub(super) async fn restore_active_credentials(
    prepared: &mut PreparedConnection,
    context: &ConnectionPollContext<'_>,
    pending_retirement: bool,
) -> bool {
    let configured = context
        .config
        .connection(&prepared.binding.connection_id)
        .is_some();
    let mode = RestoreMode::of(configured, pending_retirement);
    let binding: &RepositoryConnection = &prepared.binding;
    let scope = &prepared.scope;
    let set_id = prepared.set_id;
    let session_intent = prepared.session_intent;
    guarded_restore(
        binding.connection_id.as_str(),
        mode,
        Instant::now,
        RESTORE_ATTEMPT_DEADLINE,
        &mut prepared.restore_retry_at,
        &mut prepared.credentials,
        async || {
            restore_bound_session(scope, set_id, async || {
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
                Ok::<_, ListenFault>(SessionCredentials {
                    link,
                    session,
                    github_pat,
                })
            })
            .await
        },
    )
    .await
}

#[cfg(test)]
#[path = "credentials/tests.rs"]
mod tests;
