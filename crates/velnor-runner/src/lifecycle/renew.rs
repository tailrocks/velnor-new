// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::time::Duration;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::time::Instant;

use super::BackgroundTask;
use crate::exec::JobCancel;
use crate::protocol::{GitHubApiError, RunServiceClient, renew_failure_is_job_gone};

const RETRY_BASE: Duration = Duration::from_secs(5);
const RETRY_CAP: Duration = Duration::from_secs(20);
const LOCK_GRACE: Duration = Duration::from_secs(300);
const MIN_GAP: Duration = Duration::from_secs(1);

pub(super) type RenewHandle = BackgroundTask;

pub(super) struct RenewTarget {
    pub(super) client: RunServiceClient,
    pub(super) run_service_url: String,
    pub(super) plan_id: String,
    pub(super) job_id: String,
}

fn job_is_gone(error: &anyhow::Error) -> bool {
    renew_failure_is_job_gone(error)
        || error
            .chain()
            .find_map(|cause| cause.downcast_ref::<GitHubApiError>())
            .is_some_and(|api| api.status == 404)
}

fn retry_delay(failures: u32) -> Duration {
    RETRY_BASE
        .saturating_mul(
            1_u32
                .checked_shl(failures.saturating_sub(1))
                .unwrap_or(u32::MAX),
        )
        .min(RETRY_CAP)
}

fn until_lock_expires(locked_until: &str) -> Option<Duration> {
    let until = OffsetDateTime::parse(locked_until, &Rfc3339).ok()?;
    Some(Duration::try_from(until - OffsetDateTime::now_utc()).unwrap_or(Duration::ZERO))
}

fn next_delay(interval: Duration, remaining: Option<Duration>) -> Duration {
    let floor = interval.min(MIN_GAP);
    remaining
        .map_or(interval, |remaining| interval.min(remaining / 2))
        .max(floor)
}

pub(super) fn spawn_renew(
    target: RenewTarget,
    interval: Duration,
    cancel: JobCancel,
) -> RenewHandle {
    BackgroundTask::spawn(async move {
        let mut failures = 0_u32;
        let mut give_up_at = Instant::now() + LOCK_GRACE;
        loop {
            let delay = match target
                .client
                .renew_job(&target.run_service_url, &target.plan_id, &target.job_id)
                .await
            {
                Ok(renewed) => {
                    failures = 0;
                    let remaining = until_lock_expires(&renewed.locked_until);
                    give_up_at = Instant::now() + remaining.unwrap_or(interval) + LOCK_GRACE;
                    next_delay(interval, remaining)
                }
                Err(error) if job_is_gone(&error) => {
                    eprintln!("warning: the job is gone on GitHub, cancelling it: {error:#}");
                    cancel.cancel();
                    return;
                }
                Err(error) => {
                    failures += 1;
                    if Instant::now() >= give_up_at {
                        eprintln!("warning: job lock expired, cancelling the job: {error:#}");
                        cancel.cancel();
                        return;
                    }
                    eprintln!("warning: renew job lock failed, retrying: {error:#}");
                    retry_delay(failures)
                }
            };
            tokio::time::sleep(delay).await;
        }
    })
}
