#[cfg(test)]
use std::{path::PathBuf, sync::Arc, time::Duration};

use bollard::Docker;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Mutex, oneshot},
    time::Instant,
};

use super::recovery::recover_absent;
use super::{bounded_by_deadline, initialize};
use crate::{
    error::HostError,
    journal::{CapacityLimits, Journal, request_lifecycle::tests::helpers},
    worker::{
        WorkerIdentityNames, WorkerVolumeRole,
        storage_initialization::{
            StorageInitializationImage, StorageInitializationLimits, specification,
        },
        test_resource_budget,
    },
};

const HELPER_ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const IMAGE_PATH: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[tokio::test]
async fn sequential_steps_share_one_absolute_deadline() {
    let deadline = Instant::now() + Duration::from_millis(80);
    assert_eq!(
        bounded_by_deadline(
            deadline,
            Box::pin(tokio::time::sleep(Duration::from_millis(45))),
        )
        .await,
        Ok(())
    );
    assert_eq!(
        bounded_by_deadline(
            deadline,
            Box::pin(tokio::time::sleep(Duration::from_millis(45))),
        )
        .await,
        Err(HostError::DockerTimeout)
    );
}

#[tokio::test]
async fn initialize_keeps_helper_and_mount_backing_until_pair_handoff() -> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-adapter", 915)?;
    let scope = helpers::created_scope(&journal, &binding, 1915).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'd').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"d".repeat(32))?;
    let home_volume = names.volume_name(WorkerVolumeRole::Home);
    let work_volume = names.volume_name(WorkerVolumeRole::Work);
    let volume_identity = test_support::volume_identity(&scope, reservation_id).await?;
    let api = InitializationDockerApi::open(
        volume_identity,
        &home_volume,
        &work_volume,
        image.runtime_id(),
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'd').await?;
    let initialization = initialize(
        &api.docker,
        &scope,
        reservation_id,
        &image,
        StorageInitializationLimits::from_reserved_budget(
            test_resource_budget()?,
            100,
            128 * 1024 * 1024,
            16,
        )?,
        Duration::from_secs(20),
        &metadata,
    );
    let initialized = Box::pin(initialization).await;
    let observed = initialized?;
    assert_eq!(observed.home().inode(), 61);
    assert_eq!(observed.work().inode(), 62);
    assert_eq!(observed.claim().helper_id(), Some(HELPER_ID));
    assert_eq!(
        observed.awaiting_handoff().operation_key(),
        observed.operation_id()
    );
    let phase = helpers::storage_initializer_phase(&journal, reservation_id).await?;
    assert_eq!(phase.result, "confirmed");
    assert_eq!(phase.remove, "not_requested");
    let expected_operation = observed.operation_id().to_owned();
    assert!(
        !api.capture
            .lock()
            .await
            .requests
            .iter()
            .any(|request| { request.starts_with("DELETE /containers/") })
    );
    drop(observed);

    let capture = api.finish().await?;
    assert_initialization_request_order(&capture, &expected_operation)?;
    Ok(())
}

fn assert_initialization_request_order(
    capture: &test_support::ApiCapture,
    expected_operation: &str,
) -> Result<(), HostError> {
    let create = capture
        .requests
        .iter()
        .position(|request| request == "POST /containers/create")
        .ok_or(HostError::Docker)?;
    let attach = capture
        .requests
        .iter()
        .position(|request| {
            request.starts_with("POST /containers/") && request.ends_with("/attach")
        })
        .ok_or(HostError::Docker)?;
    let start = capture
        .requests
        .iter()
        .position(|request| request.starts_with("POST /containers/") && request.ends_with("/start"))
        .ok_or(HostError::Docker)?;
    assert!(create < attach && attach < start);
    assert!(
        !capture
            .requests
            .iter()
            .any(|request| { request.starts_with("DELETE /containers/") })
    );
    let input: Value = serde_json::from_slice(&capture.stdin).map_err(|_| HostError::Identity)?;
    assert_eq!(input["operation_id"].as_str(), Some(expected_operation));
    Ok(())
}

#[path = "tests/committed_recovery.rs"]
mod committed_recovery;
#[path = "tests/delayed_create.rs"]
mod delayed_create;
#[path = "tests/recovery.rs"]
mod recovery;
#[path = "tests/restart_recovery.rs"]
mod restart_recovery;

#[path = "tests/test_support.rs"]
pub(super) mod test_support;
use test_support::InitializationDockerApi;
