//! Physical reservation, JIT binding, and paired Docker start.

use std::future::Future;

use velnor_runner_github::{RunnerReference, Transport, jit, jit_request};

use crate::error::HostError;
use crate::journal::{
    AssignmentKey, ConnectionJournal, ReservationRequest, ReservationResult, StorageAdmission,
};
use crate::launch::resource_probe::StartObservationWitness;
use crate::launch::{Lane, SelectedEngineContext, VerifiedWorkerImages};
use crate::scale_set::EnsureError;
use crate::worker::Started;

pub(super) struct PhysicalIntent {
    pub(super) reservation_id: i64,
    intent_id: i64,
    assignment: crate::journal::AssignmentIdentity,
    runner_name: String,
    sample_generation_id: String,
    observation: StartObservationWitness,
}

pub(super) async fn reserve(
    scoped: &ConnectionJournal,
    permit: crate::launch::StartPermit,
    storage: StorageAdmission,
) -> Result<PhysicalIntent, EnsureError> {
    let nonce = fresh_nonce()?;
    let observation = permit.observation_witness();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let identity = scoped
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.clone(),
        })
        .await
        .map_err(super::super::map_journal)?;
    let runner_name = format!("v{nonce}");
    let (cpu_millicores, memory_bytes) = permit.charge();
    let engine_id = permit.engine_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let reservation = scoped
        .try_reserve_with_storage(
            ReservationRequest {
                assignment: &identity,
                engine_id: &engine_id,
                docker_root_digest: &docker_root_digest,
                cpu_millicores,
                memory_bytes,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            storage,
        )
        .await
        .map_err(super::super::map_journal)?;
    let reservation_id = match reservation {
        ReservationResult::Reserved { reservation_id } => reservation_id,
        ReservationResult::AtCapacity => {
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "host capacity",
            });
        }
        ReservationResult::Resumed { .. }
        | ReservationResult::ReconcileRequired { .. }
        | ReservationResult::Released { .. } => return Err(EnsureError::Uncertain),
    };
    let (intent_id, fresh) = scoped
        .begin_launch(&identity)
        .await
        .map_err(super::super::map_journal)?;
    if !fresh {
        return Err(EnsureError::Uncertain);
    }
    Ok(PhysicalIntent {
        reservation_id,
        intent_id,
        assignment: identity,
        runner_name,
        sample_generation_id,
        observation,
    })
}

pub(super) async fn start<T, S, F>(
    lane: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
    physical: &PhysicalIntent,
    docker: &bollard::Docker,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    start: S,
) -> Result<Started, EnsureError>
where
    T: Transport + Lane,
    S: FnOnce(&str, &[u8], crate::launch::bind::Bind, crate::stage::StageAdmission) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::super::map_journal)?;
    if physical.assignment.connection_id() != scoped.connection_id()
        || physical.assignment.connection_id() != binding.connection_id()
        || physical.assignment.scale_set_id() != binding.scale_set_id()
    {
        return Err(EnsureError::Uncertain);
    }
    let body = jit_request(&physical.runner_name).map_err(|_| EnsureError::Malformed)?;
    let claim = scoped
        .claim_runner_jit(physical.intent_id, &physical.runner_name)
        .await
        .map_err(super::super::map_journal)?
        .ok_or(EnsureError::Uncertain)?;
    if verify_effect_boundary(
        &physical.observation,
        &physical.sample_generation_id,
        docker,
        expected_engine,
        images,
    )
    .await
    .is_err()
    {
        scoped
            .mark_runner_jit_uncertain(claim)
            .await
            .map_err(super::super::map_journal)?;
        return Err(EnsureError::Uncertain);
    }
    let result = match jit(lane, physical.assignment.scale_set_id(), admin_token, &body) {
        Ok(result) => result,
        Err(error) => {
            scoped
                .mark_runner_jit_uncertain(claim)
                .await
                .map_err(super::super::map_journal)?;
            return Err(crate::listen::map_listen(error));
        }
    };
    if !runner_matches(
        &result.runner,
        physical.assignment.scale_set_id(),
        &physical.runner_name,
    ) {
        scoped
            .mark_runner_jit_uncertain(claim)
            .await
            .map_err(super::super::map_journal)?;
        return Err(EnsureError::Malformed);
    }
    scoped
        .bind_jit_runner(claim, &result.runner)
        .await
        .map_err(super::super::map_journal)?;
    start_docker(
        scoped,
        physical,
        result.encoded_jit_config.expose(),
        docker,
        expected_engine,
        images,
        start,
    )
    .await
}

async fn start_docker<S, F>(
    scoped: &ConnectionJournal,
    physical: &PhysicalIntent,
    jit_config: &str,
    docker: &bollard::Docker,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    start: S,
) -> Result<Started, EnsureError>
where
    S: FnOnce(&str, &[u8], crate::launch::bind::Bind, crate::stage::StageAdmission) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    let volume = crate::worker::new_worker_volume().map_err(|_| EnsureError::Uncertain)?;
    let claim = scoped
        .claim_physical_runner_start(physical.intent_id, &volume)
        .await
        .map_err(super::super::map_journal)?
        .ok_or(EnsureError::Uncertain)?;
    if verify_effect_boundary(
        &physical.observation,
        &physical.sample_generation_id,
        docker,
        expected_engine,
        images,
    )
    .await
    .is_err()
    {
        scoped
            .mark_physical_runner_start_uncertain(claim)
            .await
            .map_err(super::super::map_journal)?;
        return Err(EnsureError::Uncertain);
    }
    let bind = crate::launch::bind::Bind::scoped(scoped, physical.intent_id);
    let admission = crate::stage::StageAdmission::from_observation(
        &physical.observation,
        &physical.sample_generation_id,
    );
    let started = if let Ok(started) = start(&volume, jit_config.as_bytes(), bind, admission).await
    {
        started
    } else {
        scoped
            .mark_physical_runner_start_uncertain(claim)
            .await
            .map_err(super::super::map_journal)?;
        return Err(EnsureError::Uncertain);
    };
    scoped
        .record_physical_worker_ready(claim, &started.runner_id, &started.dind_id)
        .await
        .map_err(super::super::map_journal)?;
    Ok(started)
}

async fn verify_effect_boundary(
    observation: &StartObservationWitness,
    sample_generation_id: &str,
    docker: &bollard::Docker,
    expected_engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), EnsureError> {
    if !observation.is_fresh_for(
        expected_engine.engine_id(),
        expected_engine.docker_root_digest(),
        sample_generation_id,
    ) {
        return Err(EnsureError::Uncertain);
    }
    images
        .verify_for_engine(docker, expected_engine)
        .await
        .map_err(|_| EnsureError::Uncertain)?;
    if observation.is_fresh_for(
        expected_engine.engine_id(),
        expected_engine.docker_root_digest(),
        sample_generation_id,
    ) {
        Ok(())
    } else {
        Err(EnsureError::Uncertain)
    }
}

fn runner_matches(runner: &RunnerReference, set_id: i64, name: &str) -> bool {
    runner.id > 0 && runner.runner_scale_set_id == set_id && runner.name == name
}

fn fresh_nonce() -> Result<String, EnsureError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| EnsureError::Uncertain)?;
    let mut nonce = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut nonce, "{byte:02x}").map_err(|_| EnsureError::Uncertain)?;
    }
    Ok(nonce)
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
