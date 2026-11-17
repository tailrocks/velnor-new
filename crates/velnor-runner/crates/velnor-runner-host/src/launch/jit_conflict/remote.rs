// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{
    config::RepositoryConnection,
    error::HostError,
    https::HttpsTransport,
    journal::JitConflictRecoveryClaim,
    listen::{GITHUB_API, Secret},
};
use velnor_runner_github::{
    RunnerReference, RunnerResponseReceipt, Transport, observe_runner_by_name, remove_runner,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use super::ReconcileContext;

mod budget;
use budget::operation_deadline;

pub(super) async fn reconcile(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
) -> Result<bool, HostError> {
    if !context_matches_claim(context, claim) {
        return Ok(false);
    }
    context.scope.validate_jit_conflict_claim(claim).await?;
    let deadline = operation_deadline(claim)?;
    let Some(mut identity_transport) = bounded_transport(deadline) else {
        return Ok(false);
    };
    if !repository_scope_matches(&mut identity_transport, context.binding, context.github_pat) {
        return Ok(false);
    }
    let Some(mut admin_transport) = bounded_transport(deadline) else {
        return Ok(false);
    };
    admin_transport
        .set_base(context.link.admin_origin())
        .map_err(|_| HostError::Endpoint)?;
    let admin_token = Secret::new(context.link.token());
    if !reconcile_admin(
        context,
        claim,
        &mut identity_transport,
        &mut admin_transport,
        &admin_token,
    )
    .await?
    {
        return Ok(false);
    }
    context.scope.validate_jit_conflict_claim(claim).await?;
    if !context_matches_claim(context, claim)
        || !repository_scope_matches(&mut identity_transport, context.binding, context.github_pat)
    {
        return Ok(false);
    }
    context.scope.validate_jit_conflict_claim(claim).await?;
    Ok(true)
}

pub(super) async fn reconcile_admin<I, A>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity_transport: &mut I,
    transport: &mut A,
    admin_token: &Secret,
) -> Result<bool, HostError>
where
    I: Transport + ?Sized,
    A: Transport + ?Sized,
{
    reconcile_admin_with_clock(
        context,
        claim,
        identity_transport,
        transport,
        admin_token,
        || {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| HostError::Identity)
                .and_then(|duration| {
                    i64::try_from(duration.as_secs()).map_err(|_| HostError::Identity)
                })
        },
    )
    .await
}

pub(super) async fn reconcile_admin_with_clock<I, A>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity_transport: &mut I,
    transport: &mut A,
    admin_token: &Secret,
    clock: impl FnOnce() -> Result<i64, HostError>,
) -> Result<bool, HostError>
where
    I: Transport + ?Sized,
    A: Transport + ?Sized,
{
    context.scope.validate_jit_conflict_claim(claim).await?;
    if context.scope.jit_conflict_delete_started(claim).await? {
        let Some(target) = context.scope.jit_conflict_delete_target(claim).await? else {
            return Err(HostError::Identity);
        };
        return confirm_absence(
            context,
            claim,
            identity_transport,
            transport,
            admin_token,
            Some(&target),
        )
        .await;
    }

    let prior = match context
        .scope
        .jit_conflict_current_observation(claim)
        .await?
    {
        None => PriorObservation::Unobserved,
        Some(None) => PriorObservation::Absent,
        Some(Some(runner)) => PriorObservation::Found(runner),
    };
    let observed_at = clock()?;
    let Ok((observed, receipt)) = lookup(transport, claim.requested_name(), admin_token) else {
        return Ok(false);
    };
    if let Some(runner) = observed {
        reconcile_target(
            context,
            claim,
            identity_transport,
            transport,
            admin_token,
            prior,
            RunnerObservation { runner, receipt },
        )
        .await
    } else {
        if !claim.allows_initial_remote_absence()
            && !context
                .scope
                .record_jit_absent_after_deadline(claim, observed_at)
                .await?
        {
            return Ok(false);
        }
        reconcile_initial_absence(
            context,
            claim,
            identity_transport,
            transport,
            admin_token,
            prior,
            receipt,
        )
        .await
    }
}

struct RunnerObservation {
    runner: RunnerReference,
    receipt: RunnerResponseReceipt,
}

enum PriorObservation {
    Unobserved,
    Absent,
    Found(RunnerReference),
}

async fn reconcile_target<I, A>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity_transport: &mut I,
    transport: &mut A,
    admin_token: &Secret,
    prior: PriorObservation,
    observation: RunnerObservation,
) -> Result<bool, HostError>
where
    I: Transport + ?Sized,
    A: Transport + ?Sized,
{
    let RunnerObservation { runner, receipt } = observation;
    let prior_matches = match &prior {
        PriorObservation::Unobserved => true,
        PriorObservation::Absent => false,
        PriorObservation::Found(previous) => same_runner(previous, &runner),
    };
    if !same_target(&runner, claim) || !prior_matches {
        return Ok(false);
    }
    if !context_matches_claim(context, claim)
        || !repository_scope_matches(identity_transport, context.binding, context.github_pat)
    {
        return Ok(false);
    }
    if matches!(&prior, PriorObservation::Unobserved) {
        context
            .scope
            .record_jit_conflict_remote_target(claim, &runner, &receipt)
            .await?;
    }
    if !context_matches_claim(context, claim)
        || !repository_scope_matches(identity_transport, context.binding, context.github_pat)
    {
        return Ok(false);
    }
    context
        .scope
        .record_jit_conflict_delete_started(claim, &runner)
        .await?;
    if !validate_and_recheck_scope(context, claim, identity_transport).await? {
        return Ok(false);
    }
    if remove_runner(transport, runner.id, admin_token.expose()).is_err() {
        return Ok(false);
    }
    confirm_absence(
        context,
        claim,
        identity_transport,
        transport,
        admin_token,
        Some(&runner),
    )
    .await
}

async fn reconcile_initial_absence<I, A>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity_transport: &mut I,
    transport: &mut A,
    admin_token: &Secret,
    prior: PriorObservation,
    receipt: RunnerResponseReceipt,
) -> Result<bool, HostError>
where
    I: Transport + ?Sized,
    A: Transport + ?Sized,
{
    if matches!(&prior, PriorObservation::Found(_)) {
        return Ok(false);
    }
    if matches!(&prior, PriorObservation::Unobserved) {
        if !context_matches_claim(context, claim)
            || !repository_scope_matches(identity_transport, context.binding, context.github_pat)
        {
            return Ok(false);
        }
        context
            .scope
            .record_jit_conflict_remote_initial_absence(claim, &receipt)
            .await?;
    }
    confirm_absence(
        context,
        claim,
        identity_transport,
        transport,
        admin_token,
        None,
    )
    .await
}

async fn validate_and_recheck_scope<I: Transport + ?Sized>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity_transport: &mut I,
) -> Result<bool, HostError> {
    context.scope.validate_jit_conflict_claim(claim).await?;
    if !context_matches_claim(context, claim)
        || !repository_scope_matches(identity_transport, context.binding, context.github_pat)
    {
        return Ok(false);
    }
    context.scope.validate_jit_conflict_claim(claim).await?;
    Ok(true)
}

async fn confirm_absence<I, A>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity_transport: &mut I,
    transport: &mut A,
    admin_token: &Secret,
    target: Option<&RunnerReference>,
) -> Result<bool, HostError>
where
    I: Transport + ?Sized,
    A: Transport + ?Sized,
{
    context.scope.validate_jit_conflict_claim(claim).await?;
    let Ok((observed, receipt)) = lookup(transport, claim.requested_name(), admin_token) else {
        return Ok(false);
    };
    if observed.is_some() {
        if let (Some(target), Some(observed)) = (target, observed.as_ref())
            && !same_runner(target, observed)
        {
            return Ok(false);
        }
        return Ok(false);
    }
    if !context_matches_claim(context, claim) {
        return Ok(false);
    }
    if !repository_scope_matches(identity_transport, context.binding, context.github_pat) {
        return Ok(false);
    }
    context.scope.validate_jit_conflict_claim(claim).await?;
    context
        .scope
        .record_jit_conflict_remote_absence(claim, &receipt)
        .await?;
    Ok(true)
}

fn lookup<T: Transport + ?Sized>(
    transport: &mut T,
    name: &str,
    admin_token: &Secret,
) -> Result<(Option<RunnerReference>, RunnerResponseReceipt), HostError> {
    observe_runner_by_name(transport, name, admin_token.expose()).map_err(|_| HostError::Ownership)
}

fn bounded_transport(deadline: Instant) -> Option<HttpsTransport> {
    HttpsTransport::with_deadline(GITHUB_API, deadline).ok()
}

fn repository_scope_matches<T: Transport + ?Sized>(
    transport: &mut T,
    binding: &RepositoryConnection,
    github_pat: &Secret,
) -> bool {
    let Some((owner, repository)) = binding.repository.split_once('/') else {
        return false;
    };
    if owner.is_empty() || repository.is_empty() || repository.contains('/') {
        return false;
    }
    get_repository_scope(
        transport,
        RepositoryTarget {
            id: binding.repository_id,
            owner,
            name: repository,
        },
        github_pat.expose(),
    )
    .is_ok()
}

fn context_matches_claim(context: &ReconcileContext<'_>, claim: &JitConflictRecoveryClaim) -> bool {
    let expected_engine = crate::journal::EngineContext {
        engine_id: context.selected.engine_id().to_owned(),
        endpoint: context.selected.endpoint().to_owned(),
        platform: context.selected.platform().to_owned(),
    };
    claim.scale_set_id() == context.set_id
        && claim.requested_name() == format!("v{}", claim.physical_nonce())
        && context.scope.connection_id().as_str() == context.binding.connection_id.as_str()
        && claim.engine_id() == context.selected.engine_id()
        && claim.docker_root_digest() == context.selected.docker_root_digest()
        && context.selected.matches_selection(&expected_engine)
        && context.selected.matches_images(context.images)
}

fn same_target(runner: &RunnerReference, claim: &JitConflictRecoveryClaim) -> bool {
    runner.id > 0
        && runner.name == claim.requested_name()
        && runner.runner_scale_set_id == claim.scale_set_id()
}

fn same_runner(left: &RunnerReference, right: &RunnerReference) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.runner_scale_set_id == right.runner_scale_set_id
}
