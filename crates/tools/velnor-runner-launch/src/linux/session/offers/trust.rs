//! Exact Actions REST checks for the trust tuple attached to a poll event.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, timeout_at};
use velnor_runner_github::DiscoveryTransport;
use velnor_runner_github::policy::{
    ActionsWorkflowTrustRun, JobTrustEvidence, ParsedTrustBatch, VerifiedJobTrust,
    get_actions_workflow_trust_run, verify_job_offer,
};
use velnor_runner_host::BoundedDiscoveryTransport;
use zeroize::Zeroizing;

use crate::linux::session::{CancelDispatchOnDrop, DeadlineBoundTransport, DispatchFence};
use crate::linux::{LinuxLaunchContext, LinuxLaunchCredentials};

const TRUST_BUDGET: Duration = Duration::from_secs(120);
const TRUST_READS_PER_WAVE: usize = 4;

/// Fetch exact run evidence and verify each Available event against its paired
/// `jobWorkflowRef` and the immutable host policy snapshot.
pub(super) async fn verify_available(
    context: &LinuxLaunchContext,
    credentials: &LinuxLaunchCredentials,
    batch: &ParsedTrustBatch,
    stop_cutoff: Option<Instant>,
    shutdown: watch::Receiver<Option<Instant>>,
) -> Option<Vec<VerifiedJobTrust>> {
    verify_events(
        context,
        credentials,
        batch,
        |kind| matches!(kind, velnor_runner_github::InnerKind::Available),
        stop_cutoff,
        shutdown,
    )
    .await
}

/// Verify Assigned event evidence before using aggregate assigned-population
/// statistics to create generic, non-job-affine runners.
pub(super) async fn verify_assigned(
    context: &LinuxLaunchContext,
    credentials: &LinuxLaunchCredentials,
    batch: &ParsedTrustBatch,
    stop_cutoff: Option<Instant>,
    shutdown: watch::Receiver<Option<Instant>>,
) -> Option<Vec<VerifiedJobTrust>> {
    verify_events(
        context,
        credentials,
        batch,
        |kind| matches!(kind, velnor_runner_github::InnerKind::Assigned),
        stop_cutoff,
        shutdown,
    )
    .await
}

async fn verify_events(
    context: &LinuxLaunchContext,
    credentials: &LinuxLaunchCredentials,
    batch: &ParsedTrustBatch,
    include: impl Fn(&velnor_runner_github::InnerKind) -> bool,
    stop_cutoff: Option<Instant>,
    shutdown: watch::Receiver<Option<Instant>>,
) -> Option<Vec<VerifiedJobTrust>> {
    let (owner, repository) = context
        .snapshot
        .config()
        .github
        .repository
        .split_once('/')?;
    if owner.is_empty() || repository.is_empty() {
        return None;
    }

    let mut run_ids = Vec::new();
    for event in batch.events() {
        if include(&event.job().kind) {
            let run_id = event.job().workflow_run_id.filter(|value| *value > 0)?;
            if !run_ids.contains(&run_id) {
                run_ids.push(run_id);
            }
        }
    }
    let deadline = trust_deadline(stop_cutoff)?;
    if Instant::now() >= deadline {
        return None;
    }
    let runs = read_workflow_runs(
        owner,
        repository,
        &credentials.actions_read,
        &run_ids,
        deadline,
        shutdown,
    )
    .await?;

    let mut verified = Vec::new();
    for (index, event) in batch.events().iter().enumerate() {
        if !include(&event.job().kind) {
            continue;
        }
        let run_id = event.job().workflow_run_id?;
        let run = runs.get(&run_id)?;
        let evidence = context
            .snapshot
            .with_job_trust_policy_view(|policy| verify_job_offer(batch, index, run, &policy))?;
        match evidence {
            JobTrustEvidence::Verified(trust) => verified.push(*trust),
            JobTrustEvidence::Unknown(_) | JobTrustEvidence::Rejected(_) => return None,
        }
    }
    Some(verified)
}

fn trust_deadline(stop_cutoff: Option<Instant>) -> Option<Instant> {
    let budget_deadline = Instant::now().checked_add(TRUST_BUDGET)?;
    Some(stop_cutoff.map_or(budget_deadline, |cutoff| cutoff.min(budget_deadline)))
}

async fn read_workflow_runs(
    owner: &str,
    repository: &str,
    actions_token: &str,
    run_ids: &[i64],
    deadline: Instant,
    shutdown: watch::Receiver<Option<Instant>>,
) -> Option<BTreeMap<i64, ActionsWorkflowTrustRun>> {
    let mut runs = BTreeMap::new();
    for wave in run_ids.chunks(TRUST_READS_PER_WAVE) {
        if Instant::now() >= deadline {
            return None;
        }
        let mut reads = Vec::with_capacity(wave.len());
        let mut dispatches = Vec::with_capacity(wave.len());
        for run_id in wave {
            let owner = owner.to_owned();
            let repository = repository.to_owned();
            let token = Zeroizing::new(actions_token.to_owned());
            let run_id = *run_id;
            let dispatch = DispatchFence::new();
            let worker_dispatch = dispatch.clone();
            let worker_shutdown = shutdown.clone();
            dispatches.push(CancelDispatchOnDrop(dispatch));
            reads.push(tokio::task::spawn_blocking(move || {
                if !worker_dispatch.begin(&worker_shutdown, Some(deadline)) {
                    return None;
                }
                let transport = BoundedDiscoveryTransport::new();
                let mut transport = DeadlineBoundTransport::new(
                    transport,
                    worker_dispatch,
                    worker_shutdown,
                    Some(deadline),
                );
                transport.bind_github_api_origin().ok()?;
                get_actions_workflow_trust_run(&mut transport, &owner, &repository, run_id, &token)
                    .ok()
            }));
        }
        for (run_id, (read, _dispatch)) in wave.iter().zip(reads.into_iter().zip(dispatches)) {
            let result = timeout_at(TokioInstant::from_std(deadline), read)
                .await
                .ok()?
                .ok()??;
            runs.insert(*run_id, result);
        }
    }
    Some(runs)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::trust_deadline;

    #[test]
    fn trust_budget_never_extends_the_retained_shutdown_cutoff() {
        let near_cutoff = Instant::now()
            .checked_add(Duration::from_secs(30))
            .expect("monotonic clock supports a short deadline");
        assert_eq!(trust_deadline(Some(near_cutoff)), Some(near_cutoff));

        let late_cutoff = Instant::now()
            .checked_add(Duration::from_secs(180))
            .expect("monotonic clock supports a finite deadline");
        let budget_limited =
            trust_deadline(Some(late_cutoff)).expect("a finite trust budget should be available");
        assert!(budget_limited <= late_cutoff);

        let expired = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .expect("monotonic clock has a prior instant");
        assert_eq!(trust_deadline(Some(expired)), Some(expired));
    }
}
