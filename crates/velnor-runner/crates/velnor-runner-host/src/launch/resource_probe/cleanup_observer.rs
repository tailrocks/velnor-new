//! This module makes sure that the worker references are the same as the stored worker identities before the host records cleanup.

use std::time::Instant;

use bollard::Docker;
use uuid::Uuid;
use velnor_resource_probe::cleanup_observer_wire::{CleanupObserverBinding, CleanupObserverRecord};

use crate::error::HostError;
use crate::journal::{ConnectionJournal, Journal, ProbeSeed, ResourceProbePurpose};
use crate::launch::SelectedEngineContext;

#[path = "cleanup_observer/execute.rs"]
mod execute;
#[path = "cleanup_observer/record.rs"]
mod record;
#[cfg(test)]
#[path = "cleanup_observer/test_provider.rs"]
mod test_provider;

#[cfg(test)]
pub(in crate::launch) use test_provider::CleanupObserverTestProvider;

use super::lifecycle::{Deadline, bind_info, info, same_guest};
use super::provider::ReleaseImageProvider;
use super::{ReleaseReadSecret, recover};

/// These inputs share one selected host connection during observation.
#[derive(Clone, Copy)]
pub(in crate::launch) struct CleanupObserverContext<'a> {
    pub(in crate::launch) docker: &'a Docker,
    pub(in crate::launch) journal: &'a Journal,
    pub(in crate::launch) scope: &'a ConnectionJournal,
    pub(in crate::launch) selected: &'a SelectedEngineContext,
    pub(in crate::launch) release_read_secret: &'a ReleaseReadSecret,
}

/// This record has all required guest data from the selected guest.
pub(in crate::launch) struct VerifiedCleanupObserverRecord {
    reservation_id: i64,
    operation_id: String,
    engine_id: String,
    docker_root_digest: String,
    binding_sha256: String,
    record: CleanupObserverRecord,
}

impl VerifiedCleanupObserverRecord {
    pub(in crate::launch) fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(in crate::launch) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(in crate::launch) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(in crate::launch) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(in crate::launch) fn binding_sha256(&self) -> &str {
        &self.binding_sha256
    }

    pub(in crate::launch) fn record(&self) -> &CleanupObserverRecord {
        &self.record
    }
}

/// This function reads the stored worker binding and removes its specified guest probe.
pub(in crate::launch) async fn observe_cleanup_binding(
    context: CleanupObserverContext<'_>,
    reservation_id: i64,
    binding: &CleanupObserverBinding,
    deadline: Instant,
) -> Result<VerifiedCleanupObserverRecord, HostError> {
    let token = context.release_read_secret.as_provider_token()?;
    let provider =
        ReleaseImageProvider::from_compiled_source(token).map_err(|_| HostError::Docker)?;
    observe_cleanup_binding_using(context, reservation_id, binding, deadline, &provider).await
}

#[cfg(test)]
pub(in crate::launch) async fn observe_cleanup_binding_with_test_provider(
    context: CleanupObserverContext<'_>,
    reservation_id: i64,
    binding: &CleanupObserverBinding,
    deadline: Instant,
    provider: &test_provider::CleanupObserverTestProvider,
) -> Result<VerifiedCleanupObserverRecord, HostError> {
    observe_cleanup_binding_using(context, reservation_id, binding, deadline, provider).await
}

async fn observe_cleanup_binding_using<P: super::provider::ImageProvider>(
    context: CleanupObserverContext<'_>,
    reservation_id: i64,
    binding: &CleanupObserverBinding,
    deadline: Instant,
    provider: &P,
) -> Result<VerifiedCleanupObserverRecord, HostError> {
    let prepared = Box::pin(prepare(
        context,
        reservation_id,
        binding,
        deadline,
        provider,
    ))
    .await?;
    Box::pin(execute_and_verify(context, prepared)).await
}

struct PreparedObservation {
    reservation_id: i64,
    projection: super::projection::ProbeProjection,
    request_json: String,
    binding_sha256: String,
    engine_id: String,
    root: super::projection::DockerRoot,
    daemon_architecture: String,
    deadline: Deadline,
}

struct SelectedGuest {
    image: super::projection::VerifiedProbeImage,
    instance_id: String,
    engine_id: String,
    root: super::projection::DockerRoot,
    daemon_architecture: String,
    deadline: Deadline,
}

async fn prepare(
    context: CleanupObserverContext<'_>,
    reservation_id: i64,
    binding: &CleanupObserverBinding,
    deadline: Instant,
    provider: &impl super::provider::ImageProvider,
) -> Result<PreparedObservation, HostError> {
    let binding_sha256 =
        verify_stored_binding(context.scope, context.selected, reservation_id, binding).await?;
    let guest = Box::pin(prepare_selected_guest(context, deadline, provider)).await?;
    prepare_request(context, reservation_id, binding, binding_sha256, guest).await
}

async fn verify_stored_binding(
    scope: &ConnectionJournal,
    selected: &SelectedEngineContext,
    reservation_id: i64,
    binding: &CleanupObserverBinding,
) -> Result<String, HostError> {
    if reservation_id <= 0
        || velnor_resource_probe::cleanup_observer_wire::validate_binding(binding).is_err()
    {
        return Err(HostError::Identity);
    }
    let stored = scope
        .storage_cleanup_witness(reservation_id)
        .await?
        .ok_or(HostError::Identity)?;
    if stored.binding() != binding
        || stored.engine_id() != selected.engine_id()
        || stored.docker_root_digest() != selected.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    Ok(stored.binding_sha256().to_owned())
}

async fn prepare_selected_guest(
    context: CleanupObserverContext<'_>,
    deadline: Instant,
    provider: &impl super::provider::ImageProvider,
) -> Result<SelectedGuest, HostError> {
    let CleanupObserverContext {
        docker,
        journal,
        selected,
        ..
    } = context;
    let request_deadline = Deadline::until(deadline);
    let before = info(docker, &request_deadline).await?;
    let (engine_id, root, _, _) = bind_info(&before, journal, &request_deadline).await?;
    verify_selected(selected, &engine_id, &root)?;
    if let Some(active) = request_deadline.within(journal.active_probe()).await?? {
        Box::pin(recover::active(
            docker,
            journal,
            &before,
            &root,
            &request_deadline,
            active,
        ))
        .await?;
    }
    let (image, _) = super::lifecycle::verify_publication_image_until(
        docker, journal, provider, selected, deadline,
    )
    .await?;
    let daemon_architecture =
        super::provider::daemon_architecture(&before).ok_or(HostError::Identity)?;
    let selected_info = info(docker, &request_deadline).await?;
    if !same_guest(&selected_info, &engine_id, &root, daemon_architecture)? {
        return Err(HostError::Identity);
    }
    let instance_id = request_deadline.within(journal.instance_id()).await??;
    Ok(SelectedGuest {
        image,
        instance_id,
        engine_id,
        root,
        daemon_architecture: daemon_architecture.to_owned(),
        deadline: request_deadline,
    })
}

async fn prepare_request(
    context: CleanupObserverContext<'_>,
    reservation_id: i64,
    binding: &CleanupObserverBinding,
    binding_sha256: String,
    guest: SelectedGuest,
) -> Result<PreparedObservation, HostError> {
    let CleanupObserverContext { journal, scope, .. } = context;
    let SelectedGuest {
        image,
        instance_id,
        engine_id,
        root,
        daemon_architecture,
        deadline,
    } = guest;
    let operation_id = Uuid::new_v4().simple().to_string();
    let request = velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest {
        schema_version: 1,
        operation_id: operation_id.clone(),
        binding_sha256: binding_sha256.clone(),
        binding: binding.clone(),
    };
    velnor_resource_probe::cleanup_observer_wire::validate_request(&request)
        .map_err(|_| HostError::Identity)?;
    let request_json = canonical_json(&request)?;
    let request_sha256 = record::request_digest(request_json.as_bytes());
    let projection = super::projection::ProbeProjection::build_from_purpose(
        operation_id,
        instance_id.clone(),
        engine_id.clone(),
        root.path(),
        image,
        ResourceProbePurpose::CleanupObserver,
        request_sha256.clone(),
    )?;
    let seed = ProbeSeed {
        operation_id: projection.operation_id.clone(),
        instance_id,
        engine_id: projection.engine_id.clone(),
        docker_root_digest: projection.root.digest().to_owned(),
        source_revision: projection.image.source_revision().to_owned(),
        runtime_image_id: projection.image.runtime_id().to_owned(),
        image_binding_digest: projection.image.binding_fingerprint().to_owned(),
        operation_name: projection.name.clone(),
        projection_digest: projection.projection_digest.clone(),
        purpose: ResourceProbePurpose::CleanupObserver,
        request_sha256,
    };
    let persisted = deadline
        .within(journal.prepare_cleanup_observer_probe(scope, reservation_id, seed))
        .await??;
    if persisted.request() != &request
        || persisted.request_json() != request_json
        || persisted.probe().projection_digest != projection.projection_digest
    {
        let _aborted = deadline
            .within(journal.abort_prepared_probe(&projection.operation_id))
            .await;
        return Err(HostError::Identity);
    }
    Ok(PreparedObservation {
        reservation_id,
        projection,
        request_json,
        binding_sha256,
        engine_id,
        root,
        daemon_architecture,
        deadline,
    })
}

async fn execute_and_verify(
    context: CleanupObserverContext<'_>,
    prepared: PreparedObservation,
) -> Result<VerifiedCleanupObserverRecord, HostError> {
    let CleanupObserverContext {
        docker, journal, ..
    } = context;
    let result = Box::pin(execute::run(
        docker,
        journal,
        &prepared.projection,
        &prepared.request_json,
        &prepared.deadline,
    ))
    .await;
    let (record, cleanup) = result?;
    let after = info(docker, &prepared.deadline).await?;
    if !same_guest(
        &after,
        &prepared.engine_id,
        &prepared.root,
        &prepared.daemon_architecture,
    )? || cleanup.operation_id() != prepared.projection.operation_id
        || cleanup.engine_id() != prepared.engine_id
        || cleanup.docker_root_digest() != prepared.root.digest()
    {
        return Err(HostError::Identity);
    }
    Ok(VerifiedCleanupObserverRecord {
        reservation_id: prepared.reservation_id,
        operation_id: prepared.projection.operation_id,
        engine_id: prepared.engine_id,
        docker_root_digest: prepared.root.digest().to_owned(),
        binding_sha256: prepared.binding_sha256,
        record,
    })
}

fn verify_selected(
    selected: &SelectedEngineContext,
    engine_id: &str,
    root: &super::projection::DockerRoot,
) -> Result<(), HostError> {
    if selected.engine_id() == engine_id
        && selected.docker_root_digest() == root.digest()
        && selected.platform() == "linux/amd64"
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn canonical_json<T: serde::Serialize>(value: &T) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostError::Frame)?;
    if bytes.len() > 32 * 1024 {
        return Err(HostError::Frame);
    }
    String::from_utf8(bytes).map_err(|_| HostError::Frame)
}
