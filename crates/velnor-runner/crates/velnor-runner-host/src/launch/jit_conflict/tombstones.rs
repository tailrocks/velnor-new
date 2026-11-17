// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::ReconcileContext;
use crate::{
    error::HostError,
    https::HttpsTransport,
    listen::{GITHUB_API, Secret},
};
use velnor_runner_github::{Transport, observe_runner_by_name, remove_runner};

pub(super) async fn reconcile(context: &ReconcileContext<'_>) -> Result<(), HostError> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut identity =
        HttpsTransport::with_deadline(GITHUB_API, deadline).map_err(|_| HostError::Endpoint)?;
    let mut admin = HttpsTransport::with_deadline(context.link.admin_origin(), deadline)
        .map_err(|_| HostError::Endpoint)?;
    let token = Secret::new(context.link.token());
    reconcile_admin_with_clock(context, &mut identity, &mut admin, &token, || {
        i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| HostError::Identity)?
                .as_secs(),
        )
        .map_err(|_| HostError::Identity)
    })
    .await
}

pub(super) async fn reconcile_admin_with_clock<I: Transport, A: Transport>(
    context: &ReconcileContext<'_>,
    identity: &mut I,
    admin: &mut A,
    token: &Secret,
    clock: impl FnOnce() -> Result<i64, HostError>,
) -> Result<(), HostError> {
    for (reservation_id, name, set_id) in
        Box::pin(context.scope.active_jit_tombstones(clock()?)).await?
    {
        if set_id != context.set_id
            || !super::remote::repository_scope_matches(
                identity,
                context.binding,
                context.github_pat,
            )
        {
            continue;
        }
        let Ok((Some(runner), _)) = observe_runner_by_name(admin, &name, token.expose()) else {
            continue;
        };
        if runner.id <= 0
            || runner.name != name
            || runner.runner_scale_set_id != set_id
            || !super::remote::repository_scope_matches(
                identity,
                context.binding,
                context.github_pat,
            )
        {
            continue;
        }
        if remove_runner(admin, runner.id, token.expose()).is_ok() {
            context
                .scope
                .complete_jit_tombstone(reservation_id, runner.id)
                .await?;
            eprintln!(
                "The controller deleted a late JIT runner. connection={} runner={} name={}",
                context.scope.connection_id().as_str(),
                runner.id,
                name
            );
        }
    }
    Ok(())
}
