//! One-request GitHub reconciliation steps for a completed runner.

use velnor_runner_github::{RunnerReference, get_runner_by_name, remove_runner};

use super::{Context, EffectBudget, Failure, run_effect};
use crate::error::HostError;
use crate::journal::{CleanupClaim, CompletedLaunch, CompletionIdentity};

pub(super) async fn reconcile(
    context: &mut Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
) -> Result<(), Failure> {
    let identity = &launch.identity;
    let found = run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "runner lookup",
        || async {
            get_runner_by_name(
                context.transport,
                &identity.runner_name,
                context.admin.expose(),
            )
            .map_err(|_| HostError::Endpoint)
        },
    )
    .await?;
    if found
        .as_ref()
        .is_some_and(|runner| !matches_identity(runner, identity))
    {
        return Err(Failure::not_proven("runner identity"));
    }
    if found.is_some() {
        run_effect(
            context.journal,
            budget,
            launch.intent.id,
            claim,
            "runner delete",
            || async {
                remove_runner(
                    context.transport,
                    identity.runner_id,
                    context.admin.expose(),
                )
                .map_err(|_| HostError::Endpoint)
            },
        )
        .await?;
    }
    let absent = run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "runner absence lookup",
        || async {
            get_runner_by_name(
                context.transport,
                &identity.runner_name,
                context.admin.expose(),
            )
            .map_err(|_| HostError::Endpoint)
        },
    )
    .await?;
    if absent.is_some() {
        return Err(Failure::not_proven("runner absence"));
    }
    let recorded = context
        .journal
        .record_completion_runner_absent(launch.intent.id, claim.generation)
        .await
        .map_err(|error| Failure::request("runner absence record", error))?;
    if !recorded {
        return Err(Failure::not_proven("runner absence record"));
    }
    Ok(())
}

fn matches_identity(runner: &RunnerReference, identity: &CompletionIdentity) -> bool {
    runner.id == identity.runner_id
        && runner.name == identity.runner_name
        && runner.runner_scale_set_id == identity.scale_set_id
}
