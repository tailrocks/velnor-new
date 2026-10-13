//! Bounded recovery of incomplete, journal-owned worker pairs.

use std::future::Future;
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::{RemoveContainerOptionsBuilder, RemoveVolumeOptions};
use velnor_runner_github::RunnerReference;

use crate::docker_client::docker_deadline;
use crate::docker_spec::runner_plan;
use crate::error::HostError;
use crate::journal::{Journal, RecoveryLease};
use crate::worker::{WorkerVolumeRole, dind_container_name};

const RECOVERY_LEASE_SECONDS: i64 = 90;
const RECOVERY_RETRY_SECONDS: i64 = 30;

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;

/// One exact container recovered from its journal-owned name or immutable id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryContainer {
    /// Docker's immutable container id.
    pub(crate) id: String,
}

/// Ownership observation for one worker volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryVolumeState {
    /// Docker returned a confirmed not-found response.
    Absent,
    /// The exact worker and role labels match.
    Owned,
    /// The expected name exists with another identity.
    Mismatch,
}

/// One bounded Docker operation used by the recovery state machine.
pub(crate) trait RecoveryEngine {
    /// Inspect one id or exact name and validate name plus all worker labels.
    async fn inspect_worker(
        &self,
        reference: &str,
        expected_name: &str,
        worker: &str,
        role: &str,
    ) -> Result<Option<RecoveryContainer>, HostError>;
    /// Observe whether the exact container is running.
    async fn running(&self, id: &str) -> Result<bool, HostError>;
    /// Remove one immutable container id.
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    /// Inspect and verify one fixed worker-volume name.
    async fn inspect_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<RecoveryVolumeState, HostError>;
    /// Delete one fixed worker-volume name after a separate ownership check.
    async fn remove_volume(&self, worker: &str, role: WorkerVolumeRole) -> Result<(), HostError>;
}

impl RecoveryEngine for Docker {
    async fn inspect_worker(
        &self,
        reference: &str,
        expected_name: &str,
        worker: &str,
        role: &str,
    ) -> Result<Option<RecoveryContainer>, HostError> {
        match docker_deadline(self.inspect_container(reference, None)).await? {
            Ok(body) => {
                let id = body.id.filter(|id| !id.is_empty()).ok_or(HostError::Docker)?;
                let name = body.name.ok_or(HostError::Docker)?;
                let name = name.strip_prefix('/').unwrap_or(&name);
                let labels = body.config.and_then(|config| config.labels).ok_or(HostError::Docker)?;
                if name != expected_name
                    || labels.get("velnor.volume").map(String::as_str) != Some(worker)
                    || labels.get("velnor.worker").map(String::as_str) != Some(worker)
                    || labels.get("velnor.role").map(String::as_str) != Some(role)
                {
                    return Err(HostError::Docker);
                }
                Ok(Some(RecoveryContainer { id }))
            }
            Err(DockerError::DockerResponseServerError { status_code: 404, .. }) => Ok(None),
            Err(_) => Err(HostError::Docker),
        }
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        match docker_deadline(self.inspect_container(id, None)).await? {
            Ok(body) => body
                .state
                .and_then(|state| state.running)
                .ok_or(HostError::Docker),
            Err(DockerError::DockerResponseServerError { status_code: 404, .. }) => Ok(false),
            Err(_) => Err(HostError::Docker),
        }
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        docker_deadline(self.remove_container(id, Some(options)))
            .await?
            .map_err(|_| HostError::Docker)
    }

    async fn inspect_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<RecoveryVolumeState, HostError> {
        let (name, role_name) = volume_identity(worker, role)?;
        match docker_deadline(Docker::inspect_volume(self, &name)).await? {
            Ok(volume) if volume.name == name
                && volume.labels.get("velnor.worker").map(String::as_str) == Some(worker)
                && volume.labels.get("velnor.role").map(String::as_str) == Some(role_name) =>
            {
                Ok(RecoveryVolumeState::Owned)
            }
            Ok(_) => Ok(RecoveryVolumeState::Mismatch),
            Err(DockerError::DockerResponseServerError { status_code: 404, .. }) => {
                Ok(RecoveryVolumeState::Absent)
            }
            Err(_) => Err(HostError::Docker),
        }
    }

    async fn remove_volume(&self, worker: &str, role: WorkerVolumeRole) -> Result<(), HostError> {
        let (name, _) = volume_identity(worker, role)?;
        docker_deadline(Docker::remove_volume(self, &name, None::<RemoveVolumeOptions>))
            .await?
            .map_err(|_| HostError::Docker)
    }
}

/// Bounded result from one admission-boundary recovery pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RecoveryReport {
    /// Rows with complete worker, runner, and volume absence proof.
    pub(crate) cleaned: u32,
    /// Rows held for a later retry or by a competing completion owner.
    pub(crate) held: u32,
}

/// Reconcile at most four eligible journal rows before capacity admission.
pub(crate) async fn reconcile_batch<E, F>(
    journal: &Journal,
    engine: &E,
    scale_set_id: i64,
    mut runner_lookup: F,
) -> Result<RecoveryReport, HostError>
where
    E: RecoveryEngine,
    F: FnMut(&str) -> Result<Option<RunnerReference>, HostError>,
{
    let mut report = RecoveryReport::default();
    let mut claims = journal
        .claim_recovery_batch(scale_set_id, 4, RECOVERY_LEASE_SECONDS)
        .await?;
    for mut lease in claims.drain(..) {
        match recover_one(journal, engine, &mut lease, &mut runner_lookup).await {
            Ok(true) => report.cleaned = report.cleaned.saturating_add(1),
            Ok(false) | Err(_) => {
                let _released = journal
                    .release_recovery_claim(&lease, RECOVERY_RETRY_SECONDS)
                    .await?;
                report.held = report.held.saturating_add(1);
            }
        }
    }
    Ok(report)
}

async fn recover_one<E, F>(
    journal: &Journal,
    engine: &E,
    lease: &mut RecoveryLease,
    runner_lookup: &mut F,
) -> Result<bool, HostError>
where
    E: RecoveryEngine,
    F: FnMut(&str) -> Result<Option<RunnerReference>, HostError>,
{
    let worker = lease
        .intent
        .worker_volume
        .clone()
        .ok_or(HostError::Journal)?;
    let name = lease.identity.runner_name.clone();
    if !runner_absent(journal, lease, runner_lookup, &name, lease.identity.scale_set_id).await? {
        return Ok(false);
    }
    let runner = runner_plan(&worker)?;
    let dind_name = dind_container_name(&worker)?;
    let runner_id = locate_worker(
        journal,
        engine,
        lease,
        lease.intent.docker_id.clone(),
        &runner.name,
        &worker,
        "runner",
    )
    .await?;
    let dind_id = locate_worker(
        journal,
        engine,
        lease,
        lease.intent.dind_id.clone(),
        &dind_name,
        &worker,
        "dind",
    )
    .await?;
    if !journal
        .bind_recovery_containers(lease, runner_id.as_deref(), dind_id.as_deref())
        .await?
    {
        return Ok(false);
    }
    if !remove_worker(journal, engine, lease, runner_id.as_deref(), &runner.name, &worker, "runner", true).await?
        || !remove_worker(journal, engine, lease, dind_id.as_deref(), &dind.name, &worker, "dind", false).await?
    {
        return Ok(false);
    }
    if !runner_absent(journal, lease, runner_lookup, &name, lease.identity.scale_set_id).await? {
        return Ok(false);
    }
    for role in [WorkerVolumeRole::Socket, WorkerVolumeRole::Work, WorkerVolumeRole::DindData] {
        if !remove_worker_volume(journal, engine, lease, &worker, role).await? {
            return Ok(false);
        }
    }
    journal.record_recovery_cleanup(lease).await
}

async fn runner_absent<F>(
    journal: &Journal,
    lease: &mut RecoveryLease,
    lookup: &mut F,
    name: &str,
    scale_set_id: i64,
) -> Result<bool, HostError>
where
    F: FnMut(&str) -> Result<Option<RunnerReference>, HostError>,
{
    let found = request(journal, lease, || async { lookup(name) }).await?;
    match found {
        Some(runner) if runner.name == name && runner.runner_scale_set_id == scale_set_id => {
            Ok(false)
        }
        Some(_) => Err(HostError::Endpoint),
        None => Ok(true),
    }
}

async fn locate_worker<E: RecoveryEngine>(
    journal: &Journal,
    engine: &E,
    lease: &mut RecoveryLease,
    recorded_id: Option<String>,
    name: &str,
    worker: &str,
    role: &str,
) -> Result<Option<String>, HostError> {
    if let Some(recorded) = recorded_id.as_deref() {
        if let Some(found) = request(journal, lease, || {
            engine.inspect_worker(recorded, name, worker, role)
        })
        .await?
        {
            if found.id != recorded {
                return Err(HostError::Docker);
            }
            let named = request(journal, lease, || {
                engine.inspect_worker(name, name, worker, role)
            })
            .await?
            .ok_or(HostError::Docker)?;
            if named.id != recorded {
                return Err(HostError::Docker);
            }
            return Ok(Some(recorded.to_owned()));
        }
    }
    let named = request(journal, lease, || engine.inspect_worker(name, name, worker, role)).await?;
    match named {
        Some(found) if recorded_id.as_deref().is_none_or(|id| id == found.id) => {
            Ok(Some(found.id))
        }
        Some(_) => Err(HostError::Docker),
        None => Ok(None),
    }
}

async fn remove_worker<E: RecoveryEngine>(
    journal: &Journal,
    engine: &E,
    lease: &mut RecoveryLease,
    id: Option<&str>,
    name: &str,
    worker: &str,
    role: &str,
    is_runner: bool,
) -> Result<bool, HostError> {
    let Some(id) = id else {
        return request(journal, lease, || engine.inspect_worker(name, name, worker, role))
            .await
            .map(|found| found.is_none());
    };
    if is_runner && request(journal, lease, || engine.running(id)).await? {
        return Ok(false);
    }
    let current = request(journal, lease, || engine.inspect_worker(name, name, worker, role)).await?;
    match current {
        Some(current) if current.id == id => {
            request(journal, lease, || engine.remove(id)).await?;
        }
        Some(_) => return Ok(false),
        None => {}
    }
    if request(journal, lease, || engine.inspect_worker(id, name, worker, role))
        .await?
        .is_some()
        || request(journal, lease, || engine.inspect_worker(name, name, worker, role))
            .await?
            .is_some()
    {
        return Ok(false);
    }
    Ok(true)
}

async fn remove_worker_volume<E: RecoveryEngine>(
    journal: &Journal,
    engine: &E,
    lease: &mut RecoveryLease,
    worker: &str,
    role: WorkerVolumeRole,
) -> Result<bool, HostError> {
    match request(journal, lease, || engine.inspect_volume(worker, role)).await? {
        RecoveryVolumeState::Absent => Ok(true),
        RecoveryVolumeState::Mismatch => Ok(false),
        RecoveryVolumeState::Owned => {
            request(journal, lease, || engine.remove_volume(worker, role)).await?;
            Ok(request(journal, lease, || engine.inspect_volume(worker, role)).await?
                == RecoveryVolumeState::Absent)
        }
    }
}

async fn request<T, F, Fut>(
    journal: &Journal,
    lease: &mut RecoveryLease,
    operation: F,
) -> Result<T, HostError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
    if !journal
        .renew_recovery_claim(lease, RECOVERY_LEASE_SECONDS)
        .await?
    {
        return Err(HostError::Journal);
    }
    operation().await
}

fn volume_identity(
    worker: &str,
    role: WorkerVolumeRole,
) -> Result<(String, &'static str), HostError> {
    if worker.len() != 33
        || !worker.starts_with('w')
        || !worker[1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::ForbiddenMount);
    }
    let (suffix, role) = match role {
        WorkerVolumeRole::Socket => ("", "socket"),
        WorkerVolumeRole::Work => ("-work", "work"),
        WorkerVolumeRole::DindData => ("-docker", "dind-data"),
    };
    Ok((format!("{worker}{suffix}"), role))
}
