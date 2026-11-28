// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

mod acquire;
mod cancel_poll;
mod complete;
mod logs;
mod renew;
mod report;
mod session;
mod status;
mod tail;
pub mod timespan;
mod workdir;

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use reqwest::StatusCode;
use tokio::task::JoinHandle;

use crate::exec::{AdapterRegistry, JobCancel, JobResult, JobRun, RunOptions, run_job};
use crate::job_message::AgentJobRequestMessage;
use crate::protocol::{
    AcquireJobSkipReason, RetryDelays, RunnerJobRequestRef, RunnerOs, decode_jit_config,
};
use crate::secret::Secret;
use acquire::JobConnection;
use cancel_poll::CancelGrace;
use logs::{GithubSink, LogFiles};
use renew::RenewTarget;
use report::{ReportSummary, Reporter};
use session::{Assignment, Session};
use workdir::WorkDirs;

#[derive(Debug)]
pub struct RunJitOptions {
    pub jit: Secret<String>,
    pub work_dir: PathBuf,
    pub shutdown: JobCancel,
    pub renew_interval: Duration,
    pub renew_lock_grace: Duration,
    pub cancel_timeout_floor: Duration,
    pub upload_block_budget: Duration,
    pub cancel_poll_error_delay: Duration,
    pub session_retry_interval: Duration,
    pub session_close_timeout: Duration,
    pub report_budget: Duration,
    pub upload_drain_budget: Duration,
    pub feed_flush_interval: Duration,
    pub retry_delays: RetryDelays,
    pub tool_cache: Option<PathBuf>,
    pub os: RunnerOs,
    pub adapters: AdapterRegistry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOutcome {
    Completed(JobResult),
    NoJob,
}

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("JIT configuration: {0:#}")]
    Config(anyhow::Error),
    #[error("sign in to GitHub: {0:#}")]
    SignIn(anyhow::Error),
    #[error("create the broker session: {0:#}")]
    Session(anyhow::Error),
    #[error("poll the broker: {0:#}")]
    Poll(anyhow::Error),
    #[error("acquire the job: {0:#}")]
    Acquire(anyhow::Error),
    #[error("the job was not acquired: {reason:?} (HTTP {status})")]
    Skipped {
        status: StatusCode,
        reason: AcquireJobSkipReason,
    },
    #[error("prepare the work directory {}: {source}", path.display())]
    WorkDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("renew the job lock: {0:#}")]
    Renew(anyhow::Error),
    #[error("complete the job: {0:#}")]
    Complete(anyhow::Error),
}

#[derive(Debug)]
pub(super) struct BackgroundTask(JoinHandle<()>);

impl BackgroundTask {
    pub(super) fn spawn(task: impl Future<Output = ()> + Send + 'static) -> Self {
        Self(tokio::spawn(task))
    }

    pub(super) async fn finished(&mut self) {
        let _ = (&mut self.0).await;
    }
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn run_jit(options: RunJitOptions) -> Result<LifecycleOutcome, LifecycleError> {
    let config = decode_jit_config(options.jit.expose()).map_err(LifecycleError::Config)?;
    let agent_name = config.settings.agent_name.clone().unwrap_or_default();
    let mut dirs = WorkDirs::resolve(&options, &agent_name)?;
    let opened = tokio::select! {
        biased;
        () = options.shutdown.cancelled() => return Ok(LifecycleOutcome::NoJob),
        opened = Session::open(&config, options.os, options.session_retry_interval, options.retry_delays.token) => opened?,
    };
    let session = Arc::new(opened);
    let outcome = serve(&options, &session, &mut dirs).await;
    session.close(options.session_close_timeout).await;
    outcome
}

async fn serve(
    options: &RunJitOptions,
    session: &Arc<Session>,
    dirs: &mut WorkDirs,
) -> Result<LifecycleOutcome, LifecycleError> {
    let request = match session
        .wait_for_job(options.cancel_poll_error_delay, &options.shutdown)
        .await?
    {
        Assignment::Job(request) => request,
        Assignment::Shutdown => return Ok(LifecycleOutcome::NoJob),
    };
    let prepared = tokio::select! {
        biased;
        () = options.shutdown.cancelled() => return Ok(LifecycleOutcome::NoJob),
        prepared = dirs.prepare() => prepared,
    };
    let outcome = match prepared {
        Ok(files) => acquire_and_run(options, session, &dirs.run, files, &request).await,
        Err(error) => Err(error),
    };
    tokio::select! {
        biased;
        () = options.shutdown.cancelled() => dirs.remove_temp().await,
        () = dirs.remove() => {}
    }
    outcome
}

async fn acquire_and_run(
    options: &RunJitOptions,
    session: &Arc<Session>,
    run_options: &RunOptions,
    files: LogFiles,
    request: &RunnerJobRequestRef,
) -> Result<LifecycleOutcome, LifecycleError> {
    let acquired = tokio::select! {
        biased;
        () = options.shutdown.cancelled() => return Ok(LifecycleOutcome::NoJob),
        acquired = acquire::acquire(session, request, run_options.os, options.retry_delays) => acquired?,
    };
    let Some(message) = acquired else {
        return Ok(LifecycleOutcome::NoJob);
    };
    let connection = acquire::connect(&message, options.upload_block_budget, options.retry_delays)?;
    let (cancel, _shutdown_forward) = job_cancel(&options.shutdown);
    let (_renew, first_renewed) = renew::spawn_renew(
        RenewTarget {
            client: connection.run_service.clone(),
            run_service_url: connection.run_service_url.clone(),
            plan_id: message.plan.plan_id.clone(),
            job_id: message.job_id.clone(),
        },
        options.renew_interval,
        options.renew_lock_grace,
        cancel.clone(),
    );
    let lease = Lease {
        cancel,
        first_renewed,
    };
    let (completion, failure) = match run_and_report(
        options,
        session,
        run_options,
        &message,
        &connection,
        files,
        lease,
    )
    .await
    {
        Ok(Finish::Ran(run, report)) => (complete::build(&message, &run, &report), Ok(run.result)),
        Ok(Finish::Abandoned(report)) => (
            complete::abandoned(&message, &report),
            Ok(JobResult::Cancelled),
        ),
        Ok(Finish::NotStarted) => (complete::not_started(&message), Ok(JobResult::Cancelled)),
        Err(error) => (complete::forced_failure(&message, &error), Err(error)),
    };
    if let Err(error) = &failure {
        eprintln!("error: {error}");
    }
    complete::send(&connection, &completion).await?;
    failure.map(LifecycleOutcome::Completed)
}

fn job_cancel(shutdown: &JobCancel) -> (JobCancel, BackgroundTask) {
    let cancel = JobCancel::new();
    if shutdown.is_cancelled() {
        cancel.cancel();
    }
    let (shutdown, forwarded) = (shutdown.clone(), cancel.clone());
    let task = BackgroundTask::spawn(async move {
        shutdown.cancelled().await;
        forwarded.cancel();
    });
    (cancel, task)
}

struct Lease {
    cancel: JobCancel,
    first_renewed: renew::FirstRenew,
}

enum Finish {
    Ran(JobRun, ReportSummary),
    Abandoned(ReportSummary),
    NotStarted,
}

async fn cancel_deadline(cancel: &JobCancel, grace: &CancelGrace) {
    cancel.cancelled().await;
    tokio::time::sleep(grace.get()).await;
}

async fn run_and_report(
    options: &RunJitOptions,
    session: &Arc<Session>,
    run_options: &RunOptions,
    message: &AgentJobRequestMessage,
    connection: &JobConnection,
    files: LogFiles,
    lease: Lease,
) -> Result<Finish, LifecycleError> {
    let Lease {
        cancel,
        first_renewed,
    } = lease;
    let grace = CancelGrace::new(options.cancel_timeout_floor);
    let _poll = cancel_poll::spawn_cancel_poll(
        Arc::clone(session),
        message.job_id.clone(),
        cancel.clone(),
        grace.clone(),
        options.cancel_poll_error_delay,
    );
    tokio::select! {
        biased;
        () = cancel.cancelled() => return Ok(Finish::NotStarted),
        first = first_renewed => first
            .unwrap_or_else(|_| Err(anyhow!("the renew task stopped")))
            .map_err(LifecycleError::Renew)?,
    }
    let (mut sink, events) = GithubSink::channel(&files)?;
    let mut reporter = Reporter::new(
        connection,
        message,
        files,
        options.report_budget,
        options.upload_drain_budget,
        options.feed_flush_interval,
    )
    .spawn(events);
    let mut deadline = std::pin::pin!(cancel_deadline(&cancel, &grace));
    let run = tokio::select! {
        run = run_job(message, run_options, &options.adapters, &mut sink, cancel.clone()) => Some(run),
        () = deadline.as_mut() => None,
    };
    drop(sink);
    let dropped = ReportSummary {
        logs_dropped: true,
        ..ReportSummary::default()
    };
    let Some(run) = run else {
        reporter.abort();
        return Ok(Finish::Abandoned(dropped));
    };
    let report = tokio::select! {
        report = &mut reporter => report.unwrap_or_default(),
        () = deadline.as_mut() => {
            reporter.abort();
            return Ok(Finish::Abandoned(dropped));
        }
    };
    Ok(Finish::Ran(run, report))
}
