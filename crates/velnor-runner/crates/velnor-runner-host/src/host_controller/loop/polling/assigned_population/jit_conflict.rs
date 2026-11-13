// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
    host_controller::loop_driver::state::PreparedConnection,
    journal::{EngineContext, MetadataSettlementResult},
};

use super::super::PositivePollContext;

const CLAIM_LEASE_SECONDS: i64 = 240;

pub(super) async fn reconcile(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
) -> Result<bool, HostError> {
    let Some(binding) = context.config.connection(&prepared.binding.connection_id) else {
        return Ok(false);
    };
    if !prepared.is_for(binding)
        || prepared.setup.set.id != prepared.set_id
        || prepared.setup.set.name != binding.scale_set.name()
        || prepared.setup.set.labels != binding.scale_set.labels()
    {
        return Ok(false);
    }
    let engine = EngineContext {
        engine_id: context.selected_engine.engine_id().to_owned(),
        endpoint: context.config.docker.endpoint.clone(),
        platform: context.config.docker.platform.clone(),
    };
    if !context.selected_engine.matches_selection(&engine) {
        return Ok(false);
    }
    let candidates = prepared.scope.recoverable_jit_conflict_candidates().await?;
    if candidates.is_empty() {
        return Ok(false);
    }
    if prepared.credentials.is_none() {
        return Ok(false);
    }
    for candidate in candidates {
        let Some(claim) = prepared
            .scope
            .claim_jit_conflict_candidate(&candidate, CLAIM_LEASE_SECONDS)
            .await?
        else {
            continue;
        };
        if claim.scale_set_id() != prepared.set_id {
            continue;
        }
        let result = {
            let Some(credentials) = prepared.credentials.as_mut() else {
                return Ok(false);
            };
            crate::launch::jit_conflict::reconcile_claim(
                crate::launch::jit_conflict::ReconcileContext {
                    docker: context.docker,
                    journal: context.journal,
                    scope: &prepared.scope,
                    binding,
                    set_id: prepared.set_id,
                    link: &mut credentials.link,
                    github_pat: &credentials.github_pat,
                    selected: context.selected_engine,
                    images: context.worker_images,
                    image_authority: context.image_authority,
                },
                &claim,
            )
            .await
        };
        match result {
            Ok(Some(result)) => {
                if settlement_clears_population(prepared, result) {
                    return Ok(true);
                }
                eprintln!(
                    "The controller keeps a JIT-conflict reservation charged. connection={} reservation={}",
                    prepared.binding.connection_id,
                    candidate.reservation_id()
                );
            }
            Ok(None) => {}
            Err(error) => eprintln!(
                "The controller retained a JIT-conflict reservation because recovery is incomplete. connection={} reservation={} error={}",
                prepared.binding.connection_id,
                candidate.reservation_id(),
                error.code()
            ),
        }
    }
    Ok(false)
}

fn settlement_clears_population(
    prepared: &mut PreparedConnection,
    result: MetadataSettlementResult,
) -> bool {
    match result {
        MetadataSettlementResult::Settled => {
            prepared.assigned_total = None;
            true
        }
        MetadataSettlementResult::HeldAtCapacity => false,
    }
}
