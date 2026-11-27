// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;

use super::BackgroundTask;
use super::session::{MessageKind, Session, retry_poll};
use crate::exec::JobCancel;
use crate::protocol::{RunnerStatus, TaskAgentMessage};

const KILL_RESERVE: Duration = Duration::from_secs(15);
const MAX_TIMESPAN_SECONDS: u64 = 922_337_203_685;

pub(super) type CancelPollHandle = BackgroundTask;

#[derive(Debug, Clone)]
pub(super) struct CancelGrace {
    floor: Duration,
    millis: Arc<AtomicU64>,
}

impl CancelGrace {
    pub(super) fn new(floor: Duration) -> Self {
        let grace = Self {
            floor,
            millis: Arc::new(AtomicU64::new(0)),
        };
        grace.allow(floor);
        grace
    }

    fn allow(&self, timeout: Duration) {
        let kill_after = timeout.max(self.floor).saturating_sub(KILL_RESERVE);
        let millis = u64::try_from(kill_after.as_millis()).unwrap_or(u64::MAX);
        self.millis.store(millis, Ordering::SeqCst);
    }

    pub(super) fn get(&self) -> Duration {
        Duration::from_millis(self.millis.load(Ordering::SeqCst))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobCancelBody {
    #[serde(alias = "JobId")]
    job_id: String,
    #[serde(alias = "Timeout")]
    timeout: Option<String>,
}

fn parse_timespan(text: &str) -> Option<Duration> {
    let (days, clock) = match text.split_once('.') {
        Some((days, clock)) if !days.contains(':') => (days.parse::<u64>().ok()?, clock),
        _ => (0, text),
    };
    let clock = clock.split_once('.').map_or(clock, |(clock, _)| clock);
    let mut parts = clock.split(':').map(|part| part.parse::<u64>().ok());
    let (hours, minutes, seconds) = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() || hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let total = days
        .checked_mul(86_400)?
        .checked_add(hours * 3_600 + minutes * 60 + seconds)?;
    (total <= MAX_TIMESPAN_SECONDS).then(|| Duration::from_secs(total))
}

fn cancellation(message: &TaskAgentMessage, job_id: &str) -> Option<JobCancelBody> {
    if MessageKind::of(message) != MessageKind::JobCancellation {
        return None;
    }
    serde_json::from_str::<JobCancelBody>(&message.body)
        .ok()
        .filter(|body| body.job_id.eq_ignore_ascii_case(job_id))
}

pub(super) fn spawn_cancel_poll(
    session: Arc<Session>,
    job_id: String,
    cancel: JobCancel,
    grace: CancelGrace,
    error_delay: Duration,
) -> CancelPollHandle {
    BackgroundTask::spawn(async move {
        let mut streak = 0_u32;
        loop {
            match session.poll(RunnerStatus::Busy).await {
                Ok(Some(message)) => {
                    streak = 0;
                    if MessageKind::of(&message) == MessageKind::HostedRunnerShutdown {
                        cancel.cancel();
                        return;
                    }
                    if let Some(body) = cancellation(&message, &job_id) {
                        if let Some(timeout) = body.timeout.as_deref().and_then(parse_timespan) {
                            grace.allow(timeout);
                        }
                        cancel.cancel();
                        return;
                    }
                }
                Ok(None) => streak = 0,
                Err(error) => {
                    streak += 1;
                    if let Err(lost) = retry_poll(error, streak, error_delay).await {
                        eprintln!("error: {lost}; cancelling the job");
                        cancel.cancel();
                        return;
                    }
                }
            }
        }
    })
}
