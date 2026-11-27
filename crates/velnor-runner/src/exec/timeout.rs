// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::convert::Infallible;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::time::Instant;

use super::output::Output;
use super::{AnnotationLevel, JobCancel, StepResult};
use crate::expr::{ExprEnv, evaluate_condition};

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

pub(super) struct Survival<'a> {
    condition: &'a str,
    env: ExprEnv,
    verdict: OnceLock<bool>,
}

impl<'a> Survival<'a> {
    pub(super) fn new(condition: &'a str, env: ExprEnv) -> Self {
        Self {
            condition,
            env,
            verdict: OnceLock::new(),
        }
    }

    fn survives(&self) -> bool {
        *self
            .verdict
            .get_or_init(|| evaluate_condition(self.condition, &self.env) == Ok(true))
    }
}

fn interruption(
    survival: Option<&Survival<'_>>,
    job: &JobCancel,
    timeout: Option<Timeout>,
) -> Option<Interruption> {
    if job.is_cancelled() && survival.is_some_and(|survival| !survival.survives()) {
        Some(Interruption::Cancelled)
    } else if timeout.is_some_and(|timeout| Instant::now() >= timeout.at) {
        Some(if job.is_cancelled() {
            Interruption::Cancelled
        } else {
            Interruption::TimedOut
        })
    } else {
        None
    }
}

pub(super) fn stop_result(stop: Option<&JobCancel>) -> StepResult {
    if stop.is_some_and(JobCancel::is_cancelled) {
        StepResult::Cancelled
    } else {
        StepResult::Failed
    }
}

pub(super) fn settle(
    result: StepResult,
    survival: Option<&Survival<'_>>,
    job: &JobCancel,
    timeout: Option<Timeout>,
    out: &mut Output<'_>,
) -> StepResult {
    match interruption(survival, job, timeout) {
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

pub(super) async fn stop_unless_surviving(
    job: JobCancel,
    halt: JobCancel,
    survival: Option<&Survival<'_>>,
) -> Infallible {
    if let Some(survival) = survival {
        job.cancelled().await;
        if !survival.survives() {
            halt.cancel();
        }
    }
    std::future::pending().await
}
