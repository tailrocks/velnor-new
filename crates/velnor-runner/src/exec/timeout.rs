// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::convert::Infallible;
use std::time::Duration;

use tokio::sync::OnceCell;
use tokio::time::Instant;

use super::evaluate;
use super::output::Output;
use super::{AnnotationLevel, JobCancel, StepResult};
use crate::expr::{ExprEnv, ExprError};

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

pub(super) struct Survival<'a> {
    condition: &'a str,
    env: ExprEnv,
    verdict: OnceCell<Result<bool, ExprError>>,
}

impl<'a> Survival<'a> {
    pub(super) fn new(condition: &'a str, env: ExprEnv) -> Self {
        Self {
            condition,
            env,
            verdict: OnceCell::new(),
        }
    }

    async fn verdict(&self) -> &Result<bool, ExprError> {
        self.verdict
            .get_or_init(|| evaluate::condition(self.condition, &self.env))
            .await
    }

    async fn abandons(&self) -> bool {
        !matches!(self.verdict().await, Ok(true))
    }
}

pub(super) fn stop_result(stop: Option<&JobCancel>) -> StepResult {
    if stop.is_some_and(JobCancel::is_cancelled) {
        StepResult::Cancelled
    } else {
        StepResult::Failed
    }
}

pub(super) async fn settle(
    result: StepResult,
    survival: Option<&Survival<'_>>,
    job: &JobCancel,
    timeout: Option<Timeout>,
    out: &mut Output<'_>,
) -> StepResult {
    if let Some(survival) = survival.filter(|_| job.is_cancelled()) {
        if let Err(error) = survival.verdict().await {
            let text = format!("failed to evaluate the step condition: {error}");
            out.message(&text);
            out.annotate(AnnotationLevel::Error, text);
            return StepResult::Cancelled;
        }
        if survival.abandons().await {
            return StepResult::Cancelled;
        }
    }
    if timeout.is_some_and(|timeout| Instant::now() >= timeout.at) {
        if job.is_cancelled() {
            return StepResult::Cancelled;
        }
        let minutes = timeout.map_or(0.0, |timeout| timeout.minutes);
        let text = format!("step {} timed out after {minutes} minutes", out.number);
        out.message(&text);
        out.annotate(AnnotationLevel::Error, text);
        return StepResult::Failed;
    }
    result
}

pub(super) async fn stop_unless_surviving(
    job: JobCancel,
    halt: JobCancel,
    survival: Option<&Survival<'_>>,
) -> Infallible {
    if let Some(survival) = survival {
        job.cancelled().await;
        if survival.abandons().await {
            halt.cancel();
        }
    }
    std::future::pending().await
}
