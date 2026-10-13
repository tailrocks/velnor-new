use std::time::Instant;

use tokio::time::{Instant as TokioInstant, timeout_at};

use velnor_runner_host::worker::{
    DiagnosticsStore, DockerCleanupEngine, PostActionDisposition, RunnerStopPolicy,
    cleanup_worker_generation,
};
use velnor_runner_host::{
    DockerDaemonBinding, connect_unix_bound, observe_docker_daemon_binding_until,
};
use velnor_runner_journal::journal::{Journal, JournalDockerDaemonBinding};

use super::super::{LinuxAdmissionState, LinuxDaemonError, LinuxDaemonOutcome, LinuxLaunchContext};
use super::identity::cleanup_identity;
use super::summary::{outcome_with_counts, shutdown_summary, unresolved_outcome};
use crate::linux::LinuxShutdownGap;

struct CleanupInputs {
    docker: bollard::Docker,
    docker_binding: DockerDaemonBinding,
    journal_binding: JournalDockerDaemonBinding,
    rows: Vec<velnor_runner_host::IntentRow>,
    resource_count: usize,
}

pub(crate) async fn reconcile_shutdown(
    context: &LinuxLaunchContext,
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    admission: LinuxAdmissionState,
    deadline: Instant,
) -> Result<LinuxDaemonOutcome, LinuxDaemonError> {
    let inputs = match prepare_cleanup(context, journal, admission, deadline).await {
        Ok(inputs) => inputs,
        Err(outcome) => return Ok(outcome),
    };
    let engine = DockerCleanupEngine::new(&inputs.docker);
    let cleaned = cleanup_terminal_rows(
        journal,
        diagnostics,
        &engine,
        &inputs.journal_binding,
        &inputs.rows,
        deadline,
    )
    .await;
    shutdown_summary(
        journal,
        admission,
        &inputs.docker_binding,
        deadline,
        cleaned,
        inputs.resource_count,
    )
    .await
}

async fn prepare_cleanup(
    context: &LinuxLaunchContext,
    journal: &Journal,
    admission: LinuxAdmissionState,
    deadline: Instant,
) -> Result<CleanupInputs, LinuxDaemonOutcome> {
    if Instant::now() >= deadline {
        return Err(fail_cleanup(
            journal,
            admission,
            LinuxShutdownGap::DockerInventoryUnavailable,
            deadline,
        )
        .await);
    }
    let tokio_deadline = TokioInstant::from_std(deadline);
    let Ok((docker_binding, journal_binding, docker, resource_count)) =
        prepare_cleanup_docker(context, tokio_deadline).await
    else {
        return Err(fail_cleanup(
            journal,
            admission,
            LinuxShutdownGap::DockerInventoryUnavailable,
            deadline,
        )
        .await);
    };
    let Ok(Ok(rows)) = timeout_at(tokio_deadline, journal.rows()).await else {
        return Err(outcome_with_counts(
            admission,
            LinuxShutdownGap::JournalUnavailable,
            None,
            Some(resource_count),
            0,
            Some(deadline),
        ));
    };
    Ok(CleanupInputs {
        docker,
        docker_binding,
        journal_binding,
        rows,
        resource_count,
    })
}

async fn prepare_cleanup_docker(
    context: &LinuxLaunchContext,
    deadline: TokioInstant,
) -> Result<
    (
        DockerDaemonBinding,
        JournalDockerDaemonBinding,
        bollard::Docker,
        usize,
    ),
    (),
> {
    let docker_binding = timeout_at(
        deadline,
        observe_docker_daemon_binding_until(&context.docker_endpoint, deadline),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let journal_binding =
        JournalDockerDaemonBinding::new(docker_binding.endpoint(), docker_binding.engine_id())
            .map_err(|_| ())?;
    let docker = connect_unix_bound(&docker_binding).map_err(|_| ())?;
    let resource_count = read_inventory(&docker_binding, deadline).await?.len();
    Ok((docker_binding, journal_binding, docker, resource_count))
}

async fn fail_cleanup(
    journal: &Journal,
    admission: LinuxAdmissionState,
    gap: LinuxShutdownGap,
    deadline: Instant,
) -> LinuxDaemonOutcome {
    unresolved_outcome(journal, admission, gap, 0, None, deadline).await
}

async fn read_inventory(
    binding: &DockerDaemonBinding,
    deadline: TokioInstant,
) -> Result<Vec<velnor_runner_host::worker::OwnedDockerResource>, ()> {
    match timeout_at(
        deadline,
        velnor_runner_host::worker::list_owned_docker_resources_bound_until(binding, deadline),
    )
    .await
    {
        Ok(Ok(resources)) => Ok(resources),
        _ => Err(()),
    }
}

async fn cleanup_terminal_rows(
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    engine: &DockerCleanupEngine<'_>,
    binding: &JournalDockerDaemonBinding,
    rows: &[velnor_runner_host::IntentRow],
    deadline: Instant,
) -> usize {
    let mut cleaned = 0usize;
    let tokio_deadline = TokioInstant::from_std(deadline);
    for row in rows {
        if Instant::now() >= deadline {
            break;
        }
        let Some(identity) = cleanup_identity(row) else {
            continue;
        };
        if !matches!(
            timeout_at(tokio_deadline, journal.launch_daemon_binding(row.id)).await,
            Ok(Ok(Some(current))) if &current == binding
        ) {
            continue;
        }
        let grace = grace_seconds(deadline);
        if grace == 0 {
            break;
        }
        let stop = RunnerStopPolicy::StopAtDeadline {
            grace_seconds: grace,
            reason_class: "daemon_shutdown".to_owned(),
        };
        let cleanup = cleanup_worker_generation(
            engine,
            journal,
            diagnostics,
            identity,
            PostActionDisposition::Unknown,
            stop,
        );
        if matches!(timeout_at(tokio_deadline, cleanup).await, Ok(Ok(_))) {
            cleaned = cleaned.saturating_add(1);
        }
    }
    cleaned
}

/// Clean only rows backed by actual Completed event identities and the host's
/// physical cleanup proof. Callers use this after persisting lifecycle events
/// and before acknowledging their enclosing message.
pub(in crate::linux) async fn cleanup_terminal_workers(
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    docker_binding: &DockerDaemonBinding,
    deadline: Instant,
) -> Result<usize, ()> {
    if Instant::now() >= deadline {
        return Err(());
    }
    let journal_binding =
        JournalDockerDaemonBinding::new(docker_binding.endpoint(), docker_binding.engine_id())
            .map_err(|_| ())?;
    let docker = connect_unix_bound(docker_binding).map_err(|_| ())?;
    let rows = timeout_at(TokioInstant::from_std(deadline), journal.rows())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if Instant::now() >= deadline {
        return Err(());
    }
    let engine = DockerCleanupEngine::new(&docker);
    Ok(cleanup_terminal_rows(
        journal,
        diagnostics,
        &engine,
        &journal_binding,
        &rows,
        deadline,
    )
    .await)
}

fn grace_seconds(deadline: Instant) -> u32 {
    let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
    u32::try_from(remaining.min(30)).unwrap_or(u32::MAX)
}
