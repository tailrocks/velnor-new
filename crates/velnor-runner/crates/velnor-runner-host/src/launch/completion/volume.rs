//! Claim-fenced worker-volume verification, deletion, and absence proof.

use bollard::Docker;

use super::{Context, EffectBudget, Failure, run_effect};
use crate::journal::{CleanupClaim, CompletedLaunch};
use crate::stage::PairEngine;
use crate::worker::{
    VerifiedWorkerVolume, WorkerVolumeRemoval, WorkerVolumeRole, WorkerVolumeVerification,
};

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
    for role in [
        WorkerVolumeRole::Socket,
        WorkerVolumeRole::Work,
        WorkerVolumeRole::DindData,
    ] {
        let verification = verify(context, launch.intent.id, claim, budget, worker, role).await?;
        match verification {
            WorkerVolumeVerification::Absent => {}
            WorkerVolumeVerification::OwnershipMismatch => {
                return Err(Failure::not_proven("volume ownership"));
            }
            WorkerVolumeVerification::Verified(verified) => {
                remove(context, launch.intent.id, claim, budget, &verified).await?;
            }
        }
    }
    Ok(())
}

async fn verify(
    context: &Context<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    worker: &str,
    role: WorkerVolumeRole,
) -> Result<WorkerVolumeVerification, Failure> {
    let docker: &Docker = context.docker;
    run_effect(
        context.journal,
        budget,
        intent_id,
        claim,
        "volume verify",
        || async { docker.verify_volume(worker, role).await },
    )
    .await
}

async fn remove(
    context: &Context<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    verified: &VerifiedWorkerVolume,
) -> Result<(), Failure> {
    let docker: &Docker = context.docker;
    run_effect(
        context.journal,
        budget,
        intent_id,
        claim,
        "volume delete",
        || async { verified.remove_request(docker).await },
    )
    .await?;
    let removed = run_effect(
        context.journal,
        budget,
        intent_id,
        claim,
        "volume absence verify",
        || async { verified.confirm_removed(docker).await },
    )
    .await?;
    if removed == WorkerVolumeRemoval::StillPresent {
        return Err(Failure::not_proven("volume absence verify"));
    }
    Ok(())
}
