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

use std::fs::Permissions;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn run_jit(options: RunJitOptions) -> Result<LifecycleOutcome, LifecycleError> {
    let config = decode_jit_config(options.jit.expose()).map_err(LifecycleError::Config)?;
    let agent_name = config.settings.agent_name.clone().unwrap_or_default();
    let opened = tokio::select! {
        biased;
        () = options.shutdown.cancelled() => return Ok(LifecycleOutcome::NoJob),
        opened = Session::open(&config, options.os, options.session_retry_interval, options.retry_delays.token) => opened?,
    };
    let session = Arc::new(opened);
    let outcome = serve(&options, &agent_name, &session).await;
    session.close().await;
    outcome
}

async fn serve(
    options: &RunJitOptions,
    agent_name: &str,
    session: &Arc<Session>,
) -> Result<LifecycleOutcome, LifecycleError> {
    let request = match session
        .wait_for_job(options.cancel_poll_error_delay, &options.shutdown)
        .await?
    {
        Assignment::Job(request) => request,
        Assignment::Shutdown => return Ok(LifecycleOutcome::NoJob),
    };
    let root =
        std::path::absolute(&options.work_dir).map_err(|source| LifecycleError::WorkDir {
            path: options.work_dir.clone(),
            source,
        })?;
    let run_options = RunOptions {
        workspace: root.join("workspace"),
        temp: root.join("temp"),
        tool_cache: options
            .tool_cache
            .clone()
            .unwrap_or_else(|| root.join("_tool")),
        runner_name: agent_name.to_owned(),
        os: options.os,
    };
    let prepared = tokio::select! {
        biased;
        () = options.shutdown.cancelled() => return Ok(LifecycleOutcome::NoJob),
        prepared = prepare_work_dirs(&root, &run_options) => prepared,
    };
    let outcome = match prepared {
        Ok(files) => acquire_and_run(options, session, &run_options, files, &request).await,
        Err(error) => Err(error),
    };
    tokio::select! {
        biased;
        () = options.shutdown.cancelled() => remove_temp(&run_options).await,
        () = remove_work_dirs(&root, &run_options) => {}
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
    let (completion, failure) =
        match run_and_report(options, session, run_options, &message, &connection, files).await {
            Ok(Finish::Ran(run, report)) => {
                (complete::build(&message, &run, &report), Ok(run.result))
            }
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

async fn remove_work_dirs(root: &Path, options: &RunOptions) {
    let logs = LogFiles::dir_of(root);
    for directory in [&options.workspace, &options.temp, &logs] {
        if let Err(error) = remove_dir(directory).await {
            eprintln!("warning: remove {}: {error}", directory.display());
        }
    }
    if let Some(parent) = options.workspace.parent() {
        let _ = tokio::fs::remove_dir(parent).await;
    }
}

async fn remove_temp(options: &RunOptions) {
    if let Err(error) = remove_dir(&options.temp).await {
        eprintln!("warning: remove {}: {error}", options.temp.display());
    }
}

async fn remove_dir(directory: &Path) -> std::io::Result<()> {
    match tokio::fs::remove_dir_all(directory).await {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

const PRIVATE_DIR_MODE: u32 = 0o700;
const WORK_DIR_MODE: u32 = 0o755;

async fn create_work_root(root: &Path) -> std::io::Result<()> {
    if tokio::fs::try_exists(root).await? {
        return Ok(());
    }
    tokio::fs::create_dir_all(root).await?;
    tokio::fs::set_permissions(root, Permissions::from_mode(WORK_DIR_MODE)).await
}

async fn fresh_dir(directory: &Path, mode: Option<u32>) -> std::io::Result<()> {
    remove_dir(directory).await?;
    tokio::fs::create_dir_all(directory).await?;
    match mode {
        Some(mode) => tokio::fs::set_permissions(directory, Permissions::from_mode(mode)).await,
        None => Ok(()),
    }
}

async fn prepare_work_dirs(root: &Path, options: &RunOptions) -> Result<LogFiles, LifecycleError> {
    let logs = LogFiles::dir_of(root);
    let work_dir = |path: &Path, source| LifecycleError::WorkDir {
        path: path.to_path_buf(),
        source,
    };
    create_work_root(root)
        .await
        .map_err(|source| work_dir(root, source))?;
    for (directory, mode) in [
        (&options.workspace, None),
        (&options.temp, Some(PRIVATE_DIR_MODE)),
        (&logs, Some(PRIVATE_DIR_MODE)),
    ] {
        fresh_dir(directory, mode)
            .await
            .map_err(|source| work_dir(directory, source))?;
    }
    Ok(LogFiles::at(logs))
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
) -> Result<Finish, LifecycleError> {
    let (cancel, _shutdown_forward) = job_cancel(&options.shutdown);
    let grace = CancelGrace::new(options.cancel_timeout_floor);
    let _poll = cancel_poll::spawn_cancel_poll(
        Arc::clone(session),
        message.job_id.clone(),
        cancel.clone(),
        grace.clone(),
        options.cancel_poll_error_delay,
    );
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
