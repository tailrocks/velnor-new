//! Claim-fenced cleanup orchestration and one-request effect boundaries.

#[path = "container.rs"]
mod container;
#[path = "runner.rs"]
mod runner;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
#[path = "volume.rs"]
mod volume;

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use bollard::Docker;

use super::Resources;
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::{CleanupClaim, CompletedLaunch, Journal};
use crate::listen::Secret;

const CLAIM_LEASE_SECONDS: i64 = 90;
#[cfg(test)]
const CURL_REQUEST_TIMEOUT_SECONDS: u64 = 60;
#[cfg(test)]
const EFFECT_MARGIN_SECONDS: u64 = 30;
const EFFECT_WINDOW_SECONDS: i64 = 90;
const RUNNER_REQUESTS: u64 = 3;
const CONTAINER_COUNT: u64 = 2;
const CONTAINER_REQUESTS: u64 = 6;
const VOLUME_COUNT: u64 = 3;
const VOLUME_REQUESTS: u64 = 3;
const MAX_CLEANUP_EFFECTS: u64 =
    RUNNER_REQUESTS + CONTAINER_COUNT * CONTAINER_REQUESTS + VOLUME_COUNT * VOLUME_REQUESTS;
#[cfg(test)]
const MAX_EXTERNAL_CHAIN_SECONDS: u64 = RUNNER_REQUESTS * CURL_REQUEST_TIMEOUT_SECONDS
    + (CONTAINER_COUNT * CONTAINER_REQUESTS + VOLUME_COUNT * VOLUME_REQUESTS)
        * crate::docker_client::DOCKER_OPERATION_TIMEOUT.as_secs();
const SCAN_LIMIT: u32 = 4;
const MAX_SCAN_WAVES: usize = 3;
const SCAN_INTERVAL: Duration = Duration::from_secs(30);

type HostResult<T> = Result<T, HostError>;

pub(super) struct Context<'a> {
    pub(super) journal: &'a Journal,
    pub(super) docker: &'a Docker,
    pub(super) transport: &'a mut HttpsTransport,
    pub(super) admin: &'a Secret,
}

/// One bounded reconciliation chain: three GitHub calls, twelve container
/// calls, and nine volume calls at most. Each HTTP call renews the durable
/// lease independently; no serial chain shares a single effect window.
pub(super) struct EffectBudget<'a> {
    remaining: u64,
    stopping: &'a AtomicBool,
}

impl<'a> EffectBudget<'a> {
    pub(super) fn new(stopping: &'a AtomicBool) -> Self {
        Self {
            remaining: MAX_CLEANUP_EFFECTS,
            stopping,
        }
    }

    fn reserve(&mut self, phase: &'static str) -> Result<(), Failure> {
        if self.remaining == 0 {
            return Err(Failure::bounded(phase));
        }
        self.remaining -= 1;
        Ok(())
    }
}

/// A bounded, non-secret cleanup failure suitable for logs and shutdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) phase: &'static str,
    pub(super) reason: &'static str,
    pub(super) error: HostError,
    pub(super) stop_requested: bool,
}

impl Failure {
    pub(super) const fn request(phase: &'static str, error: HostError) -> Self {
        Self {
            phase,
            reason: error_reason(error),
            error,
            stop_requested: false,
        }
    }

    pub(super) const fn not_proven(phase: &'static str) -> Self {
        Self {
            phase,
            reason: "not_proven",
            error: HostError::Journal,
            stop_requested: false,
        }
    }

    pub(super) const fn claim_lost() -> Self {
        Self {
            phase: "claim renewal",
            reason: "claim_lost",
            error: HostError::Journal,
            stop_requested: false,
        }
    }

    const fn stopped(phase: &'static str) -> Self {
        Self {
            phase,
            reason: "stopping",
            error: HostError::Journal,
            stop_requested: true,
        }
    }

    const fn bounded(phase: &'static str) -> Self {
        Self {
            phase,
            reason: "effect_bound",
            error: HostError::Journal,
            stop_requested: false,
        }
    }

    pub(super) fn report(self) {
        eprintln!(
            "completion reconciliation failed phase={} reason={}",
            self.phase, self.reason
        );
    }
}

const fn error_reason(error: HostError) -> &'static str {
    match error {
        HostError::Docker => "docker",
        HostError::Endpoint => "endpoint",
        HostError::Journal => "journal",
        _ => "host",
    }
}

pub(super) fn run(
    runtime: &tokio::runtime::Runtime,
    resources: Resources,
    stopping: &Arc<AtomicBool>,
    receiver: &Receiver<()>,
) -> Result<(), HostError> {
    let Resources {
        journal,
        docker,
        mut transport,
        admin,
    } = resources;
    let mut context = Context {
        journal: &journal,
        docker: &docker,
        transport: &mut transport,
        admin: &admin,
    };
    let mut first_failure = None;
    loop {
        if stopping.load(Ordering::Acquire) {
            return first_failure.map_or(Ok(()), Err);
        }
        match runtime.block_on(reconcile_due_burst(&mut context, stopping)) {
            Ok((_, failure)) => remember_failure(&mut first_failure, failure),
            Err(failure) => {
                failure.report();
                remember_failure(&mut first_failure, Some(failure));
            }
        }
        if stopping.load(Ordering::Acquire) {
            return first_failure.map_or(Ok(()), Err);
        }
        match receiver.recv_timeout(SCAN_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return first_failure.map_or(Ok(()), Err);
            }
        }
    }
}

fn remember_failure(first: &mut Option<HostError>, failure: Option<Failure>) {
    if first.is_none()
        && let Some(failure) = failure
    {
        *first = Some(failure.error);
    }
}

async fn reconcile_due_burst(
    context: &mut Context<'_>,
    stopping: &AtomicBool,
) -> Result<(usize, Option<Failure>), Failure> {
    let scan_now = completion_now().map_err(|error| Failure::request("due scan", error))?;
    let page_size = usize::try_from(SCAN_LIMIT)
        .map_err(|_| Failure::request("due scan", HostError::Journal))?;
    let mut processed = 0;
    let mut first_failure = None;
    for _ in 0..MAX_SCAN_WAVES {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let due = context
            .journal
            .due_completed_launches(scan_now, SCAN_LIMIT)
            .await
            .map_err(|error| Failure::request("due scan", error))?;
        let count = due.len();
        if count == 0 {
            break;
        }
        for launch in due {
            if stopping.load(Ordering::Acquire) {
                return Ok((processed, first_failure));
            }
            if let Some(failure) = reconcile_one(context, launch, stopping).await
                && first_failure.is_none()
            {
                first_failure = Some(failure);
            }
            processed += 1;
        }
        if count < page_size {
            break;
        }
    }
    Ok((processed, first_failure))
}

async fn reconcile_one(
    context: &mut Context<'_>,
    launch: CompletedLaunch,
    stopping: &AtomicBool,
) -> Option<Failure> {
    let claim = match context
        .journal
        .claim_completion_cleanup(launch.intent.id, CLAIM_LEASE_SECONDS)
        .await
    {
        Ok(Some(claim)) => claim,
        Ok(None) => return None,
        Err(error) => {
            let failure = Failure::request("claim", error);
            failure.report();
            return Some(failure);
        }
    };
    let mut budget = EffectBudget::new(stopping);
    let result = reconcile_claimed(context, &launch, claim, &mut budget).await;
    let Err(failure) = result else {
        return None;
    };
    if !failure.stop_requested {
        failure.report();
    }
    // Retry scheduling clears the lease, so retain the request window as a
    // retry delay when a Docker or curl response may have been lost.
    match context
        .journal
        .retry_completion_cleanup(
            launch.intent.id,
            claim.generation,
            cleanup_retry_delay_seconds(claim.attempt),
        )
        .await
    {
        Ok(true) if failure.stop_requested => None,
        Ok(true) => Some(failure),
        Ok(false) => {
            let lost = Failure::claim_lost();
            lost.report();
            Some(lost)
        }
        Err(error) => {
            let retry = Failure::request("retry schedule", error);
            retry.report();
            Some(retry)
        }
    }
}

async fn reconcile_claimed(
    context: &mut Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
) -> Result<(), Failure> {
    if !valid_completion(launch) {
        return Err(Failure::not_proven("completion identity"));
    }
    runner::reconcile(context, launch, claim, budget).await?;
    container::cleanup(context, launch, claim, budget).await?;
    volume::cleanup(context, launch, claim, budget).await?;
    let recorded = context
        .journal
        .record_completion_cleanup(launch.intent.id, claim.generation)
        .await
        .map_err(|error| Failure::request("cleanup proof", error))?;
    if !recorded {
        return Err(Failure::not_proven("cleanup proof"));
    }
    Ok(())
}

/// Renew before one HTTP request. A request closure must contain exactly one
/// Docker Engine or curl exchange; serial requests have separate renewals.
pub(super) async fn run_effect<T, F, Fut>(
    journal: &Journal,
    budget: &mut EffectBudget<'_>,
    intent_id: i64,
    claim: CleanupClaim,
    phase: &'static str,
    effect: F,
) -> Result<T, Failure>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = HostResult<T>>,
{
    if budget.stopping.load(Ordering::Acquire) {
        return Err(Failure::stopped(phase));
    }
    budget.reserve(phase)?;
    let stopping = budget.stopping;
    let outcome = journal
        .run_completion_cleanup_effect(
            intent_id,
            claim.generation,
            EFFECT_WINDOW_SECONDS,
            || async {
                if stopping.load(Ordering::Acquire) {
                    Ok(EffectOutcome::Stopped)
                } else {
                    Ok(EffectOutcome::Completed(effect().await))
                }
            },
        )
        .await
        .map_err(|error| Failure::request("claim renewal", error))?;
    match outcome {
        Some(EffectOutcome::Stopped) => Err(Failure::stopped(phase)),
        Some(EffectOutcome::Completed(Ok(value))) => Ok(value),
        Some(EffectOutcome::Completed(Err(error))) => Err(Failure::request(phase, error)),
        None => Err(Failure::claim_lost()),
    }
}

enum EffectOutcome<T> {
    Stopped,
    Completed(HostResult<T>),
}

fn valid_completion(launch: &CompletedLaunch) -> bool {
    launch.intent.id > 0
        && launch.intent.kind == "launch"
        && !launch.intent.cleanup_proven
        && launch.identity.scale_set_id > 0
        && launch.identity.runner_request_id > 0
        && launch.identity.runner_id > 0
        && !launch.identity.runner_name.is_empty()
        && launch
            .intent
            .worker_volume
            .as_deref()
            .is_some_and(|worker| crate::runner_plan(worker).is_ok())
}

fn retry_delay_seconds(attempt: u32) -> i64 {
    let mut delay = 5_i64;
    for _ in 1..attempt.min(6) {
        delay = delay.saturating_mul(2);
    }
    delay.min(300)
}

fn cleanup_retry_delay_seconds(attempt: u32) -> i64 {
    EFFECT_WINDOW_SECONDS.saturating_add(retry_delay_seconds(attempt))
}

fn completion_now() -> HostResult<i64> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}
