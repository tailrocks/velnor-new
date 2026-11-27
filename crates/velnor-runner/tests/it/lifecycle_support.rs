// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use velnor_runner::exec::{AdapterRegistry, JobCancel};
use velnor_runner::lifecycle::{LifecycleError, LifecycleOutcome, RunJitOptions, run_jit};
use velnor_runner::protocol::{RetryDelays, RunnerOs};
use velnor_runner::secret::Secret;

use super::exec_support::{Scratch, fixture_path};
use super::support::mock_github::MockGithub;

pub(crate) const RUN_BUDGET: Duration = Duration::from_secs(60);
pub(crate) const CANCEL_BUDGET: Duration = Duration::from_secs(15);

pub(crate) fn fixture_job(name: &str, script: Option<&str>) -> Result<Value> {
    job_replacing(name, script.map(|script| ("echo hello", script)))
}

fn replace_literal(value: &mut Value, from: &str, to: &str) -> bool {
    match value {
        Value::String(text) if text == from => {
            to.clone_into(text);
            true
        }
        Value::Array(items) => items.iter_mut().any(|item| replace_literal(item, from, to)),
        Value::Object(fields) => fields
            .values_mut()
            .any(|field| replace_literal(field, from, to)),
        _ => false,
    }
}

pub(crate) fn job_replacing(name: &str, replacement: Option<(&str, &str)>) -> Result<Value> {
    let text = std::fs::read_to_string(fixture_path(name)).context("read the fixture")?;
    let mut job: Value = serde_json::from_str(&text).context("parse the fixture")?;
    if let Some((from, to)) = replacement {
        anyhow::ensure!(
            replace_literal(&mut job, from, to),
            "{name} has no {from:?}"
        );
    }
    Ok(job)
}

pub(crate) fn job_with_steps(count: usize) -> Result<Value> {
    let mut job = fixture_job("hello", None)?;
    let steps = job["Steps"].as_array_mut().context("fixture steps")?;
    let step = steps.first().cloned().context("fixture step")?;
    *steps = vec![step; count];
    Ok(job)
}

#[derive(Clone)]
pub(crate) struct Settings {
    pub(crate) renew_interval: Duration,
    pub(crate) renew_lock_grace: Duration,
    pub(crate) cancel_timeout_floor: Duration,
    pub(crate) upload_block_budget: Duration,
    pub(crate) session_retry_interval: Duration,
    pub(crate) poll_error_delay: Duration,
    pub(crate) report_budget: Duration,
    pub(crate) upload_drain_budget: Duration,
    pub(crate) feed_flush_interval: Duration,
    pub(crate) os: RunnerOs,
    pub(crate) budget: Duration,
    pub(crate) shutdown: JobCancel,
}

impl Settings {
    pub(crate) fn new() -> Self {
        Self {
            renew_interval: Duration::from_millis(200),
            renew_lock_grace: Duration::from_secs(300),
            cancel_timeout_floor: Duration::from_secs(60),
            upload_block_budget: Duration::from_secs(60),
            session_retry_interval: Duration::from_millis(50),
            poll_error_delay: Duration::from_millis(50),
            report_budget: Duration::from_secs(10),
            upload_drain_budget: Duration::from_secs(600),
            feed_flush_interval: Duration::from_millis(250),
            os: RunnerOs::Linux,
            budget: RUN_BUDGET,
            shutdown: JobCancel::new(),
        }
    }

    pub(crate) fn renewing_every(mut self, interval: Duration) -> Self {
        self.renew_interval = interval;
        self
    }

    pub(crate) fn giving_up_after(mut self, grace: Duration) -> Self {
        self.renew_lock_grace = grace;
        self
    }

    pub(crate) fn on(mut self, os: RunnerOs) -> Self {
        self.os = os;
        self
    }

    pub(crate) fn retrying_sessions_every(mut self, interval: Duration) -> Self {
        self.session_retry_interval = interval;
        self
    }

    pub(crate) fn retrying_polls_every(mut self, delay: Duration) -> Self {
        self.poll_error_delay = delay;
        self
    }

    pub(crate) fn killing_after_cancel_floor(mut self, floor: Duration) -> Self {
        self.cancel_timeout_floor = floor;
        self
    }

    pub(crate) fn uploading_each_block_within(mut self, budget: Duration) -> Self {
        self.upload_block_budget = budget;
        self
    }

    pub(crate) fn reporting_within(mut self, budget: Duration) -> Self {
        self.report_budget = budget;
        self
    }

    pub(crate) fn draining_uploads_within(mut self, budget: Duration) -> Self {
        self.upload_drain_budget = budget;
        self
    }

    pub(crate) fn flushing_the_feed_every(mut self, interval: Duration) -> Self {
        self.feed_flush_interval = interval;
        self
    }

    pub(crate) fn within(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }
}

pub(crate) async fn run_in(
    mock: &MockGithub,
    scratch: &Scratch,
    settings: Settings,
) -> Result<Result<LifecycleOutcome, LifecycleError>> {
    let budget = settings.budget;
    let options = RunJitOptions {
        jit: Secret::new(mock.jit().to_owned()),
        work_dir: scratch.work_dir(),
        shutdown: settings.shutdown,
        renew_interval: settings.renew_interval,
        renew_lock_grace: settings.renew_lock_grace,
        cancel_timeout_floor: settings.cancel_timeout_floor,
        upload_block_budget: settings.upload_block_budget,
        cancel_poll_error_delay: settings.poll_error_delay,
        session_retry_interval: settings.session_retry_interval,
        report_budget: settings.report_budget,
        upload_drain_budget: settings.upload_drain_budget,
        feed_flush_interval: settings.feed_flush_interval,
        retry_delays: RetryDelays::IMMEDIATE,
        os: settings.os,
        adapters: AdapterRegistry::empty(),
    };
    tokio::time::timeout(budget, run_jit(options))
        .await
        .with_context(|| format!("run_jit finished within {budget:?}"))
}

pub(crate) async fn run_with(mock: &MockGithub, settings: Settings) -> Result<LifecycleOutcome> {
    let scratch = Scratch::new()?;
    let outcome = run_in(mock, &scratch, settings).await??;
    assert!(
        !scratch.work_dir().join("workspace").exists(),
        "the workspace is removed after the job"
    );
    assert!(
        !scratch.work_dir().join("logs").exists(),
        "the log spill files are removed after the job"
    );
    Ok(outcome)
}

pub(crate) fn shutdown_after(delay: Duration) -> JobCancel {
    let shutdown = JobCancel::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        signal.cancel();
    });
    shutdown
}

pub(crate) async fn run(mock: &MockGithub, renew_interval: Duration) -> Result<LifecycleOutcome> {
    run_with(mock, Settings::new().renewing_every(renew_interval)).await
}
