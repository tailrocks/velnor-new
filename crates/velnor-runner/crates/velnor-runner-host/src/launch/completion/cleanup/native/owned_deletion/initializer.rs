// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
    journal::{CleanupClaim, CompletedLaunch, StorageCleanupIdentity, UnboundInitializer},
    launch::bootstrap::{InitializerPlan, plan_initializer_removal},
};

use super::super::super::super::{Context, EffectBudget, Failure};
use super::helpers::stop_helper;
use super::{inspect_container, remove_container_and_check, request_effect};

pub(super) async fn settle_journaled_initializer(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
) -> Result<(), Failure> {
    let scope = context.journal.connection();
    let Some(helper) = request_effect(
        context,
        launch,
        claim,
        budget,
        "initializer journal lookup",
        identity,
        || async {
            scope
                .unretired_storage_initializer(
                    identity.reservation_id(),
                    identity.engine_id(),
                    identity.docker_root_digest(),
                )
                .await
        },
    )
    .await?
    else {
        return Ok(());
    };
    let by_name = request_effect(
        context,
        launch,
        claim,
        budget,
        "initializer name absence",
        identity,
        || async { inspect_container(context.docker, &helper.name).await },
    )
    .await?;
    let by_id = match helper.id.as_deref() {
        Some(id) => {
            request_effect(
                context,
                launch,
                claim,
                budget,
                "initializer id absence",
                identity,
                || async { inspect_container(context.docker, id).await },
            )
            .await?
        }
        None => None,
    };
    match plan_initializer_removal(&helper, by_name.as_ref(), by_id.as_ref()) {
        Ok(InitializerPlan::Absent) => {
            settle_removed(context, launch, claim, budget, identity, &helper).await
        }
        Ok(InitializerPlan::Remove { id }) => {
            let running = by_name
                .as_ref()
                .and_then(|container| container.state.as_ref())
                .and_then(|state| state.running)
                == Some(true);
            remove_matched_initializer(
                context,
                launch,
                claim,
                budget,
                identity,
                &helper,
                (&id, running),
            )
            .await
        }
        Err(HostError::Ownership) => Err(Failure::not_proven("volume helper ownership")),
        Err(error) => Err(Failure::request("initializer identity", error)),
    }
}

async fn remove_matched_initializer(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    helper: &UnboundInitializer,
    removal: (&str, bool),
) -> Result<(), Failure> {
    let (id, running) = removal;
    if running {
        request_effect(
            context,
            launch,
            claim,
            budget,
            "initializer stop",
            identity,
            || async { stop_helper(context, id).await },
        )
        .await?;
    }
    remove_container_and_check(context, launch, claim, budget, id, identity).await?;
    let name_left = request_effect(
        context,
        launch,
        claim,
        budget,
        "initializer name absence",
        identity,
        || async { inspect_container(context.docker, &helper.name).await },
    )
    .await?;
    let id_left = match helper.id.as_deref() {
        Some(saved) => {
            request_effect(
                context,
                launch,
                claim,
                budget,
                "initializer id absence",
                identity,
                || async { inspect_container(context.docker, saved).await },
            )
            .await?
        }
        None => None,
    };
    if name_left.is_some() || id_left.is_some() {
        return Err(Failure::not_proven("initializer absence"));
    }
    settle_removed(context, launch, claim, budget, identity, helper).await
}

async fn settle_removed(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    helper: &UnboundInitializer,
) -> Result<(), Failure> {
    let scope = context.journal.connection();
    request_effect(
        context,
        launch,
        claim,
        budget,
        "initializer journal settlement",
        identity,
        || async {
            scope
                .settle_removed_storage_initializer(
                    identity.reservation_id(),
                    identity.engine_id(),
                    identity.docker_root_digest(),
                    helper,
                )
                .await
        },
    )
    .await
}
