//! Claim-fenced container identity checks and deletion.

use bollard::Docker;

use super::{Context, EffectBudget, Failure, run_effect};
use crate::journal::{CleanupClaim, CompletedLaunch};
use crate::runner_plan;
use crate::stage::PairEngine;
use crate::worker::dind_create;

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;

pub(super) async fn cleanup(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
) -> Result<(), Failure> {
    let worker = launch
        .intent
        .worker_volume
        .as_deref()
        .ok_or_else(|| Failure::not_proven("worker identity"))?;
    let runner = runner_plan(worker).map_err(|error| Failure::request("runner plan", error))?;
    let dind = dind_create(worker).map_err(|error| Failure::request("dind plan", error))?;
    let tasks = [
        ContainerCleanup {
            recorded_id: launch.intent.docker_id.as_deref(),
            name: &runner.name,
            volume: worker,
            role: "runner",
        },
        ContainerCleanup {
            recorded_id: launch.intent.dind_id.as_deref(),
            name: &dind.name,
            volume: worker,
            role: "dind",
        },
    ];
    for task in tasks {
        cleanup_container(context, launch.intent.id, claim, budget, task).await?;
    }
    Ok(())
}

struct ContainerCleanup<'a> {
    recorded_id: Option<&'a str>,
    name: &'a str,
    volume: &'a str,
    role: &'a str,
}

async fn cleanup_container(
    context: &Context<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    task: ContainerCleanup<'_>,
) -> Result<(), Failure> {
    let Some(owned_id) = observe_worker(context, intent_id, claim, budget, &task).await? else {
        return Ok(());
    };
    if task.recorded_id.is_none() {
        let binding = match task.role {
            "runner" => (Some(owned_id.as_str()), None),
            "dind" => (None, Some(owned_id.as_str())),
            _ => return Err(Failure::not_proven("container role")),
        };
        let bound = context
            .journal
            .bind_completion_containers(intent_id, claim.generation, binding.0, binding.1)
            .await
            .map_err(|error| Failure::request("container id record", error))?;
        if !bound {
            return Err(Failure::not_proven("container id record"));
        }
    }
    let current = request_worker_id(context, intent_id, claim, budget, &owned_id, &task).await?;
    match current.as_deref() {
        Some(current_id) if current_id != owned_id => {
            return Err(Failure::not_proven("container identity"));
        }
        Some(_) => {
            remove_container(context, intent_id, claim, budget, &owned_id).await?;
        }
        None => {}
    }
    if request_worker_id(context, intent_id, claim, budget, &owned_id, &task)
        .await?
        .is_some()
    {
        return Err(Failure::not_proven("container id absence"));
    }
    if request_worker_id(context, intent_id, claim, budget, task.name, &task)
        .await?
        .is_some()
    {
        return Err(Failure::not_proven("container name absence"));
    }
    Ok(())
}

async fn observe_worker(
    context: &Context<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    task: &ContainerCleanup<'_>,
) -> Result<Option<String>, Failure> {
    if let Some(recorded) = task.recorded_id
        && let Some(found) =
            request_worker_id(context, intent_id, claim, budget, recorded, task).await?
    {
        if found != recorded {
            return Err(Failure::not_proven("container id identity"));
        }
        return Ok(Some(found));
    }
    let found = request_worker_id(context, intent_id, claim, budget, task.name, task).await?;
    if let Some(recorded) = task.recorded_id {
        match found.as_deref() {
            Some(found) if found != recorded => {
                return Err(Failure::not_proven("container name identity"));
            }
            Some(_) => {}
            None => return Ok(None),
        }
    }
    Ok(found)
}

async fn request_worker_id(
    context: &Context<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    name: &str,
    task: &ContainerCleanup<'_>,
) -> Result<Option<String>, Failure> {
    let docker: &Docker = context.docker;
    run_effect(
        context.journal,
        budget,
        intent_id,
        claim,
        "container inspect",
        || async {
            docker
                .worker_id_for_name(name, task.volume, task.role)
                .await
        },
    )
    .await
}

async fn remove_container(
    context: &Context<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    id: &str,
) -> Result<(), Failure> {
    let docker = context.docker;
    run_effect(
        context.journal,
        budget,
        intent_id,
        claim,
        "container delete",
        || async { docker.remove(id).await },
    )
    .await
}
