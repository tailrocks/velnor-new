//! Claim-fenced cleanup orchestration and one-request effect boundaries.

mod native;
#[path = "runner.rs"]
mod runner;
#[cfg(test)]
#[path = "cleanup/scoped_shutdown_tests.rs"]
mod scoped_shutdown_tests;
#[path = "cleanup/service_loop.rs"]
mod service_loop;

pub(in crate::launch) use native::remove_owned_resources_for_retirement;

#[cfg(test)]
pub(crate) use runner::reconcile_for_test;
pub(super) use service_loop::run;

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use bollard::Docker;

use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::{CleanupClaim, CompletedLaunch};
use crate::launch::SelectedEngineContext;
use crate::launch::bootstrap::NativeCleanupRuntime;
use crate::listen::Secret;

use super::journal_ref::CompletionJournal;

const CLAIM_LEASE_SECONDS: i64 = 90;
const EFFECT_WINDOW_SECONDS: i64 = 90;
const RUNNER_REQUESTS: u64 = 3;
const CONTAINER_COUNT: u64 = 2;
const VOLUME_COUNT: u64 = crate::worker::WorkerVolumeRole::ALL.len() as u64;
const MAX_NATIVE_CLEANUP_EFFECTS: u64 =
    RUNNER_REQUESTS + CONTAINER_COUNT * 4 * 2 + VOLUME_COUNT * 3 * 2 + 7;
const SCAN_LIMIT: u32 = 4;
const MAX_SCAN_WAVES: usize = 3;

type HostResult<T> = Result<T, HostError>;

pub(super) struct Context<'a> {
    pub(super) journal: &'a CompletionJournal,
    pub(super) docker: &'a Docker,
    pub(super) transport: &'a mut HttpsTransport,
    pub(super) admin: &'a Secret,
    pub(super) diagnostics: &'a super::diagnostics::DiagnosticsRoot,
    pub(super) native_cleanup: Option<&'a NativeCleanupRuntime>,
    pub(super) selected_engine: Option<&'a SelectedEngineContext>,
}

/// One bounded cleanup uses three GitHub calls, eighteen container calls, and three calls per volume role.
/// Each call renews its durable lease independently.
pub(super) struct EffectBudget<'a> {
    remaining: u64,
    stopping: &'a AtomicBool,
}

impl<'a> EffectBudget<'a> {
    pub(super) fn new(stopping: &'a AtomicBool) -> Self {
        Self {
            remaining: MAX_NATIVE_CLEANUP_EFFECTS,
            stopping,
        }
    }

    #[cfg(test)]
    fn new_native(stopping: &'a AtomicBool) -> Self {
        Self::new(stopping)
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
    if context
        .diagnostics
        .pending_for(launch.intent.id)
        .map_err(|error| Failure::request("diagnostic worker state", error))?
    {
        return Err(Failure::not_proven("diagnostic write pending"));
    }
    native::cleanup(context, launch, claim, budget).await?;
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
    journal: &CompletionJournal,
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
