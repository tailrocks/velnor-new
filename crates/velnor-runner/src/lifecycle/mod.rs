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

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use tokio::task::JoinHandle;

use crate::exec::{AdapterRegistry, JobCancel, JobResult, JobRun, RunOptions, run_job};
use crate::job_message::AgentJobRequestMessage;
use crate::protocol::{AcquireJobSkipReason, decode_jit_config};
use acquire::JobConnection;
use logs::GithubSink;
use renew::RenewTarget;
use report::Reporter;
use session::{Assignment, Session};

#[derive(Debug)]
pub struct RunJitOptions {
    pub jit: String,
    pub work_dir: PathBuf,
    pub renew_interval: Duration,
    pub cancel_poll_error_delay: Duration,
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
    #[error("wait for a job: {0:#}")]
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
    let config = decode_jit_config(&options.jit).map_err(LifecycleError::Config)?;
    let agent_name = config.settings.agent_name.clone().unwrap_or_default();
    let session = Arc::new(Session::open(&config).await?);
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
        .wait_for_job(options.cancel_poll_error_delay)
        .await?
    {
        Assignment::Job(request) => request,
        Assignment::Shutdown => return Ok(LifecycleOutcome::NoJob),
    };
    let message = acquire::acquire(session, &request).await?;
    let connection = acquire::connect(&message)?;
    let root =
        std::path::absolute(&options.work_dir).map_err(|source| LifecycleError::WorkDir {
            path: options.work_dir.clone(),
            source,
        })?;
    let run_options = RunOptions {
        workspace: root.join("workspace"),
        temp: root.join("temp"),
        runner_name: agent_name.to_owned(),
    };
    let outcome = execute(options, session, &run_options, &message, &connection).await;
    remove_work_dirs(&run_options).await;
    outcome.map(LifecycleOutcome::Completed)
}

async fn remove_work_dirs(options: &RunOptions) {
    for directory in [&options.workspace, &options.temp] {
        if let Err(error) = remove_dir(directory).await {
            eprintln!("warning: remove {}: {error}", directory.display());
        }
    }
    if let Some(parent) = options.workspace.parent() {
        let _ = tokio::fs::remove_dir(parent).await;
    }
}

async fn remove_dir(directory: &Path) -> std::io::Result<()> {
    match tokio::fs::remove_dir_all(directory).await {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

async fn prepare_work_dirs(options: &RunOptions) -> Result<(), LifecycleError> {
    for directory in [&options.workspace, &options.temp] {
        let created = match remove_dir(directory).await {
            Ok(()) => tokio::fs::create_dir_all(directory).await,
            Err(error) => Err(error),
        };
        created.map_err(|source| LifecycleError::WorkDir {
            path: directory.clone(),
            source,
        })?;
    }
    Ok(())
}

async fn execute(
    options: &RunJitOptions,
    session: &Arc<Session>,
    run_options: &RunOptions,
    message: &AgentJobRequestMessage,
    connection: &JobConnection,
) -> Result<JobResult, LifecycleError> {
    prepare_work_dirs(run_options).await?;
    let cancel = JobCancel::new();
    let renew = renew::spawn_renew(
        RenewTarget {
            client: connection.run_service.clone(),
            run_service_url: connection.run_service_url.clone(),
            plan_id: message.plan.plan_id.clone(),
            job_id: message.job_id.clone(),
        },
        options.renew_interval,
        cancel.clone(),
    );
    let poll = cancel_poll::spawn_cancel_poll(
        Arc::clone(session),
        message.job_id.clone(),
        cancel.clone(),
        options.cancel_poll_error_delay,
    );
    let (mut sink, events) = GithubSink::channel();
    let reporter = Reporter::new(connection, message).spawn(events);
    let run: JobRun = run_job(message, run_options, &options.adapters, &mut sink, cancel).await;
    drop(poll);
    drop(sink);
    let report = reporter.await.unwrap_or_default();
    let completion = complete::build(message, &run, &report);
    complete::send(connection, &completion).await?;
    drop(renew);
    Ok(run.result)
}
