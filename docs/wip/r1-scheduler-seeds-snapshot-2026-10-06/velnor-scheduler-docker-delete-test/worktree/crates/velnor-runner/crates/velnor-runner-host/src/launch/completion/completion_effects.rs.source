//! Claim-authorized worker and volume cleanup effects.

use std::future::Future;

use crate::action_archive_seed::ActionArchiveLease;
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::stage::{ObservedWorker, PairEngine, WorkerVolume, reconcile_worker};

pub(super) async fn cleanup_owned_pair<E: PairEngine>(
    journal: &Journal,
    docker: &E,
    identity: &crate::journal::LaunchIdentity,
    observed: &ObservedWorker,
    row: &IntentRow,
    archive_lease: Option<&ActionArchiveLease>,
    generation: i64,
) -> Result<(), &'static str> {
    let runner_id = observed.runner_id().or(row.docker_id.as_deref());
    let dind_id = observed.dind_id().or(row.dind_id.as_deref());
    if let Some(runner_id) = runner_id {
        run_docker_cleanup_effect(journal, row.id, generation, || async {
            remove_container_effect(
                journal,
                docker,
                identity,
                "runner",
                runner_id,
                dind_id,
                archive_lease,
                row.id,
                generation,
            )
            .await
        })
        .await?;
    }
    if let Some(dind_id) = dind_id {
        run_docker_cleanup_effect(journal, row.id, generation, || async {
            remove_container_effect(
                journal,
                docker,
                identity,
                "dind",
                dind_id,
                None,
                None,
                row.id,
                generation,
            )
            .await
        })
        .await?;
    }
    verify_pair_absent(journal, docker, identity, row.id, generation).await?;
    for volume in WorkerVolume::CLEANUP_ORDER {
        run_docker_cleanup_effect(journal, row.id, generation, || async {
            remove_volume_effect(journal, docker, identity, volume, row.id, generation).await
        })
        .await?;
    }
    Ok(())
}

async fn remove_container_effect<E: PairEngine>(
    journal: &Journal,
    docker: &E,
    identity: &crate::journal::LaunchIdentity,
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
    row_id: i64,
    generation: i64,
) -> Result<(), &'static str> {
    let observed = reconcile_worker(
        docker,
        identity,
        (role == "runner").then_some(id),
        (role == "dind").then_some(id),
        archive_lease,
    )
    .await
    .map_err(|_| "docker-cleanup")?;
    let observed_id = match role {
        "runner" => observed.runner_id(),
        "dind" => observed.dind_id(),
        _ => return Err("docker-cleanup"),
    };
    if observed_id.is_none() {
        // Exact-label listing, deterministic-name inspection, and the recorded-ID
        // inspection all returned confirmed absence. This makes a lost remove
        // response safe to reconcile after restart.
        return Ok(());
    }
    verify_container_identity(role, id, dind_id, archive_lease, &observed)?;
    authorize_next_effect(journal, row_id, generation).await?;
    docker.remove(id).await.map_err(|_| "docker-cleanup")?;
    if docker
        .inspect_container(id)
        .await
        .map_err(|_| "docker-cleanup")?
        .is_some()
    {
        return Err("docker-cleanup");
    }
    Ok(())
}

fn verify_container_identity(
    role: &str,
    id: &str,
    dind_id: Option<&str>,
    archive_lease: Option<&ActionArchiveLease>,
    observed: &ObservedWorker,
) -> Result<(), &'static str> {
    match role {
        "runner" if observed.runner_id() == Some(id) => {
            if observed.dind_id() != dind_id {
                return Err("docker-cleanup");
            }
            match observed.runner_running() {
                Some(false) => Ok(()),
                Some(true) | None => Err("docker-cleanup"),
            }
        }
        "dind" if observed.dind_id() == Some(id) => {
            if observed.runner_id().is_some() || dind_id.is_some() || archive_lease.is_some() {
                return Err("docker-cleanup");
            }
            Ok(())
        }
        _ => Err("docker-cleanup"),
    }
}

async fn remove_volume_effect<E: PairEngine>(
    journal: &Journal,
    docker: &E,
    identity: &crate::journal::LaunchIdentity,
    volume: WorkerVolume,
    row_id: i64,
    generation: i64,
) -> Result<(), &'static str> {
    docker
        .verify_volume(identity, volume)
        .await
        .map_err(|_| "docker-cleanup")?;
    authorize_next_effect(journal, row_id, generation).await?;
    docker
        .remove_volume(identity, volume)
        .await
        .map_err(|_| "docker-cleanup")
}

async fn verify_pair_absent<E: PairEngine>(
    journal: &Journal,
    docker: &E,
    identity: &crate::journal::LaunchIdentity,
    row_id: i64,
    generation: i64,
) -> Result<(), &'static str> {
    authorize_next_effect(journal, row_id, generation).await?;
    let remaining = reconcile_worker(docker, identity, None, None, None)
        .await
        .map_err(|_| "docker-cleanup")?;
    if remaining.runner_id().is_some() || remaining.dind_id().is_some() {
        return Err("docker-cleanup");
    }
    Ok(())
}

async fn authorize_next_effect(
    journal: &Journal,
    row_id: i64,
    generation: i64,
) -> Result<(), &'static str> {
    if journal
        .completion_cleanup_claim_current(row_id, generation)
        .await
        .map_err(|_| "cleanup-claim-check")?
    {
        Ok(())
    } else {
        Err("cleanup-claim-expired")
    }
}

async fn run_docker_cleanup_effect<F, Fut>(
    journal: &Journal,
    row_id: i64,
    generation: i64,
    effect: F,
) -> Result<(), &'static str>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), &'static str>>,
{
    let result = journal
        .run_completion_cleanup_effect(row_id, generation, || async { Ok(effect().await) })
        .await
        .map_err(|_| "cleanup-claim-check")?;
    match result {
        Some(Ok(())) => Ok(()),
        Some(Err(stage)) => Err(stage),
        None => Err("cleanup-claim-expired"),
    }
}
