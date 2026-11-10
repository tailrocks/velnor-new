// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::RunnerReference;

use super::{Context, EffectBudget, Failure, run_effect};
use crate::journal::{CleanupClaim, CompletedLaunch, CompletionIdentity};

pub(super) async fn reconcile(
    context: &mut Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
) -> Result<(), Failure> {
    let mut session = context
        .admin
        .session()
        .map_err(|error| Failure::request("runner credentials", error))?;
    let journal = context.journal;

    let identity = &launch.identity;
    let found = run_effect(
        journal,
        budget,
        launch.intent.id,
        claim,
        "runner lookup",
        || async { session.lookup(&identity.runner_name) },
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
            journal,
            budget,
            launch.intent.id,
            claim,
            "runner delete",
            || async { session.remove(identity.runner_id) },
        )
        .await?;
    }
    let absent = run_effect(
        journal,
        budget,
        launch.intent.id,
        claim,
        "runner absence lookup",
        || async { session.lookup(&identity.runner_name) },
    )
    .await?;
    if absent.is_some() {
        return Err(Failure::not_proven("runner absence"));
    }
    let recorded = journal
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
