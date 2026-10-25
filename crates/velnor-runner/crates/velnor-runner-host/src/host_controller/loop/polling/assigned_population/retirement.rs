//! Recover only orphaned physical attempts selected by the durable journal.

use crate::{
    error::HostError, host_controller::loop_driver::state::PreparedConnection,
    launch::SelectedEngineContext,
};

use super::super::PositivePollContext;

const CLAIM_LEASE_SECONDS: i64 = 60;

pub(super) async fn reconcile(prepared: &PreparedConnection, context: &PositivePollContext<'_>) {
    let Ok(candidates) = prepared.scope.recoverable_post_jit_retirements().await else {
        return;
    };
    let Some(binding) = context.config.connection(&prepared.binding.connection_id) else {
        return;
    };
    let engine = crate::journal::EngineContext {
        engine_id: context.selected_engine.engine_id().to_owned(),
        endpoint: context.config.docker.endpoint.clone(),
        platform: context.config.docker.platform.clone(),
    };
    let selected = context.selected_engine;
    if !selected.matches_selection(&engine) {
        return;
    }
    for candidate in candidates {
        let result = Box::pin(reconcile_candidate(
            prepared, context, binding, selected, candidate,
        ))
        .await;
        if result.is_err() {
            eprintln!(
                "The controller retained capacity while post-JIT recovery remains incomplete. connection={}",
                prepared.binding.connection_id
            );
        }
    }
}

async fn reconcile_candidate(
    prepared: &PreparedConnection,
    context: &PositivePollContext<'_>,
    binding: &crate::config::RepositoryConnection,
    selected: &SelectedEngineContext,
    candidate: crate::journal::PostJitRetirementCandidate,
) -> Result<(), HostError> {
    let Some(claim) = prepared
        .scope
        .claim_recoverable_post_jit_retirement(&candidate, CLAIM_LEASE_SECONDS)
        .await?
    else {
        return Ok(());
    };
    let runtime_result = Box::pin(crate::launch::retirement::reconcile_claim(
        crate::launch::retirement::RetirementContext {
            docker: context.docker,
            journal: context.journal,
            scope: &prepared.scope,
            binding,
            selected,
            images: context.worker_images,
            release_secret: context.release_read_secret,
            claim: &claim,
        },
    ))
    .await?;
    if runtime_result == crate::journal::MetadataSettlementResult::HeldAtCapacity {
        eprintln!(
            "The controller keeps the recovered runner capacity charged. connection={} reservation={}",
            prepared.binding.connection_id,
            candidate.reservation_id()
        );
    }
    Ok(())
}
