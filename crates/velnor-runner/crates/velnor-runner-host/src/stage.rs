// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::RemoveContainerOptionsBuilder;

use crate::docker_client::docker_deadline;
use crate::docker_spec::{DeleteDecision, delete_decision, runner_plan};
use crate::error::HostError;
use crate::worker::{
    CreateProjection, ResourceBudget, WorkerVolumeRole, WorkerVolumeVerification,
    create_named_volumes, create_only, deliver_jit, dind_create, join_dind_net,
    remove_worker_volumes, runner_create, start_id, verify_worker_volume,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartialPair {
    pub dind_id: Option<String>,

    pub runner_id: Option<String>,
}

pub(crate) trait PairEngine {
    async fn prepare_volumes(&self, volume: &str, runner_image: &str) -> Result<(), HostError>;
    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError>;
    async fn start(&self, id: &str) -> Result<(), HostError>;
    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError>;
    async fn remove(&self, id: &str) -> Result<(), HostError>;
    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError>;
    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
        engine_id: &str,
        docker_root: &str,
        image_id: &str,
    ) -> Result<Option<String>, HostError>;
    async fn verify_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError>;
    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError>;
    async fn running(&self, id: &str) -> Result<bool, HostError>;
}

pub(crate) trait PairSink {
    async fn volume(&self, volume: &str) -> Result<(), HostError>;
    async fn dind(&self, id: &str) -> Result<(), HostError>;
    async fn runner(&self, id: &str) -> Result<(), HostError>;
}

impl PairEngine for Docker {
    async fn prepare_volumes(&self, volume: &str, runner_image: &str) -> Result<(), HostError> {
        let mounts = crate::worker::dind_mounts(volume)?;
        Box::pin(docker_deadline(create_named_volumes(self, volume, &mounts))).await??;
        crate::work_owner::own_work_volume(self, &format!("{volume}-work"), runner_image).await
    }

    async fn create(&self, spec: &CreateProjection) -> Result<String, HostError> {
        Box::pin(docker_deadline(create_only(self, spec))).await?
    }

    async fn start(&self, id: &str) -> Result<(), HostError> {
        Box::pin(docker_deadline(start_id(self, id))).await?
    }

    async fn write_jit(&self, id: &str, jit: &[u8]) -> Result<(), HostError> {
        Box::pin(docker_deadline(deliver_jit(self, id, jit))).await?
    }

    async fn remove(&self, id: &str) -> Result<(), HostError> {
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        Box::pin(docker_deadline(self.remove_container(id, Some(options))))
            .await?
            .map_err(|_| HostError::Docker)
    }

    async fn id_for_name(&self, name: &str) -> Result<Option<String>, HostError> {
        if name.is_empty() {
            return Err(HostError::Docker);
        }
        match Box::pin(docker_deadline(self.inspect_container(name, None))).await? {
            Ok(body) => body
                .id
                .filter(|id| !id.is_empty())
                .map(Some)
                .ok_or(HostError::Docker),
            Err(DockerError::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(_) => Err(HostError::Docker),
        }
    }

    async fn worker_id_for_name(
        &self,
        name: &str,
        volume: &str,
        role: &str,
        engine_id: &str,
        docker_root: &str,
        image_id: &str,
    ) -> Result<Option<String>, HostError> {
        crate::worker::worker_id_for_name(
            self,
            name,
            volume,
            role,
            engine_id,
            docker_root,
            image_id,
        )
        .await
    }

    async fn verify_volume(
        &self,
        worker: &str,
        role: WorkerVolumeRole,
    ) -> Result<WorkerVolumeVerification, HostError> {
        verify_worker_volume(self, worker, role).await
    }

    async fn remove_worker_volumes(&self, volume: &str) -> Result<bool, HostError> {
        remove_worker_volumes(self, volume).await
    }

    async fn running(&self, id: &str) -> Result<bool, HostError> {
        let response = Box::pin(docker_deadline(self.inspect_container(id, None))).await?;
        match crate::launch::classify_inspect(response) {
            Ok(running) => Ok(running),
            Err(_) => Err(HostError::Docker),
        }
    }
}

pub(crate) async fn drive<E: PairEngine, S: PairSink>(
    engine: &E,
    private_volume: &str,
    resource_budget: ResourceBudget,
    jit: &[u8],
    sink: &S,
    images: &crate::AdmittedImages,
) -> Result<PartialPair, HostError> {
    if jit.is_empty() {
        return Err(HostError::EmptyJit);
    }
    let runner = runner_create(
        &runner_plan(private_volume, images)?,
        resource_budget,
        images,
    )?;
    let dind = dind_create(private_volume, resource_budget, images)?;
    sink.volume(private_volume).await?;
    engine
        .prepare_volumes(private_volume, &images.runner)
        .await?;
    let dind_id = engine.create(&dind).await?;
    if let Err(error) = sink.dind(&dind_id).await {
        return drop_id(engine, &dind_id, error).await;
    }
    start_or_drop(engine, &dind_id).await?;
    let spec = join_or_drop(engine, runner, &dind_id).await?;
    let runner_id = match engine.create(&spec).await {
        Ok(id) => id,
        Err(error) => drop_id(engine, &dind_id, error).await?,
    };
    if let Err(error) = sink.runner(&runner_id).await {
        return drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if let Err(error) = engine.start(&runner_id).await {
        return drop_both(engine, &dind_id, &runner_id, error).await;
    }
    if let Err(error) = engine.write_jit(&runner_id, jit).await {
        return drop_both(engine, &dind_id, &runner_id, error).await;
    }
    Ok(PartialPair::both(dind_id, runner_id))
}

pub async fn remove_recorded(
    docker: &Docker,
    journal_path: &std::path::Path,
    intent_id: i64,
) -> Result<DeleteDecision, HostError> {
    let info = Box::pin(docker_deadline(docker.info()))
        .await?
        .map_err(|_| HostError::Docker)?;
    let engine_id = info.id.ok_or(HostError::Docker)?;
    let path = crate::daemon_lock::canonical_journal_path(journal_path)?;
    let guard = crate::daemon_lock::EngineLineageGuard::acquire(&engine_id)?;
    let journal = crate::journal::Journal::open(&path).await?;
    journal.bind_engine(&engine_id).await?;
    let instance = journal.instance_id().await?;
    guard.verify_lineage(&path, &instance, journal.revision().await?)?;
    journal.attach_lineage_guard(guard.clone())?;
    let row = journal
        .rows()
        .await?
        .into_iter()
        .find(|row| row.id == intent_id)
        .ok_or(HostError::Journal)?;
    if row.cleanup_proven {
        return Ok(DeleteDecision::NotDeleted);
    }
    crate::launch::slot::release_row(&journal, docker, &row)
        .await
        .map_err(|_| HostError::Ownership)?;
    let updated = journal
        .rows()
        .await?
        .into_iter()
        .find(|row| row.id == intent_id)
        .ok_or(HostError::Journal)?;
    guard.advance_revision(journal.revision().await?)?;
    Ok(if updated.cleanup_proven {
        DeleteDecision::Delete
    } else {
        DeleteDecision::KeepForeign
    })
}

pub(crate) async fn decide<E: PairEngine + ?Sized>(
    engine: &E,
    owned_id: &str,
    name: &str,
) -> Result<DeleteDecision, HostError> {
    let observed = engine.id_for_name(name).await?;
    let decision = delete_decision(owned_id, observed.as_deref());
    if decision == DeleteDecision::Delete {
        engine.remove(owned_id).await?;
    }
    Ok(decision)
}

async fn start_or_drop<E: PairEngine>(engine: &E, id: &str) -> Result<(), HostError> {
    if let Err(error) = engine.start(id).await {
        return drop_id(engine, id, error).await;
    }
    Ok(())
}

async fn join_or_drop<E: PairEngine>(
    engine: &E,
    runner: CreateProjection,
    dind_id: &str,
) -> Result<CreateProjection, HostError> {
    match join_dind_net(runner, dind_id) {
        Ok(spec) => Ok(spec),
        Err(error) => drop_id(engine, dind_id, error).await,
    }
}

async fn drop_id<E: PairEngine, T>(engine: &E, id: &str, error: HostError) -> Result<T, HostError> {
    let _kept = decide(engine, id, id).await.err();
    Err(error)
}

async fn drop_both<E: PairEngine, T>(
    engine: &E,
    dind_id: &str,
    runner_id: &str,
    error: HostError,
) -> Result<T, HostError> {
    let _runner = decide(engine, runner_id, runner_id).await.err();
    drop_id(engine, dind_id, error).await
}

impl PartialPair {
    fn both(dind_id: String, runner_id: String) -> Self {
        Self {
            dind_id: Some(dind_id),
            runner_id: Some(runner_id),
        }
    }
}
