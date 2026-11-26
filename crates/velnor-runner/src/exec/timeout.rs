// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::time::Instant;

use super::output::Output;
use super::{AnnotationLevel, JobCancel, StepResult};

#[derive(Clone, Copy)]
pub(super) struct Timeout {
    pub(super) minutes: f64,
    at: Instant,
}

impl Timeout {
    pub(super) fn start(minutes: Option<f64>) -> Option<Self> {
        let minutes = minutes?;
        let limit = Duration::try_from_secs_f64(minutes * 60.0).ok()?;
        Some(Self {
            minutes,
            at: Instant::now().checked_add(limit)?,
        })
    }
}

async fn expired(timeout: Option<Timeout>) {
    match timeout {
        Some(timeout) => tokio::time::sleep_until(timeout.at).await,
        None => std::future::pending().await,
    }
}

async fn cancelled(cancel: Option<&JobCancel>) {
    match cancel {
        Some(cancel) => cancel.cancelled().await,
        None => std::future::pending().await,
    }
}

pub(super) async fn interrupted(cancel: Option<&JobCancel>, timeout: Option<Timeout>) {
    tokio::select! {
        () = cancelled(cancel) => {}
        () = expired(timeout) => {}
    }
}

enum Interruption {
    Cancelled,
    TimedOut,
}

fn interruption(cancel: Option<&JobCancel>, timeout: Option<Timeout>) -> Option<Interruption> {
    if cancel.is_some_and(JobCancel::is_cancelled) {
        Some(Interruption::Cancelled)
    } else if timeout.is_some_and(|timeout| Instant::now() >= timeout.at) {
        Some(Interruption::TimedOut)
    } else {
        None
    }
}

pub(super) fn stop_result(cancel: Option<&JobCancel>, timeout: Option<Timeout>) -> StepResult {
    match interruption(cancel, timeout) {
        Some(Interruption::Cancelled) => StepResult::Cancelled,
        _ => StepResult::Failed,
    }
}

pub(super) fn settle(
    result: StepResult,
    cancel: Option<&JobCancel>,
    timeout: Option<Timeout>,
    out: &mut Output<'_>,
) -> StepResult {
    match interruption(cancel, timeout) {
        None => result,
        Some(Interruption::Cancelled) => StepResult::Cancelled,
        Some(Interruption::TimedOut) => {
            let minutes = timeout.map_or(0.0, |timeout| timeout.minutes);
            let text = format!("step {} timed out after {minutes} minutes", out.number);
            out.message(&text);
            out.annotate(AnnotationLevel::Error, text);
            StepResult::Failed
        }
    }
}
