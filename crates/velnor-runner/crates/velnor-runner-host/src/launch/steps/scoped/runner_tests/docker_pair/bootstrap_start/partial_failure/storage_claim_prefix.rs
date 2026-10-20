use super::*;
use crate::journal::StorageRole;

#[path = "storage_claim_prefix/assertions.rs"]
mod assertions;

use assertions::{
    assert_created_prefix, assert_failure_error, assert_failure_response,
    assert_no_container_create,
};

pub(super) struct PreparationFailure {
    pub(super) api: PairDockerApi,
    pub(super) error: String,
    pub(super) ownership_error: bool,
    pub(super) expected_volume: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum FailurePoint {
    JournalClaim,
    DockerInspect,
    ExistingOwnedVolume,
    InspectStatus(u16),
}

impl PreparationFailure {
    pub(super) async fn finish(self) -> Result<CapturedApi, String> {
        self.api.finish().await
    }
}

#[tokio::test]
async fn production_claim_and_inspect_failures_stop_later_storage_creates() -> Result<(), String> {
    Box::pin(assert_storage_prefix_stops_at_each_role(
        FailurePoint::JournalClaim,
    ))
    .await?;
    Box::pin(assert_storage_prefix_stops_at_each_role(
        FailurePoint::DockerInspect,
    ))
    .await?;
    Box::pin(assert_storage_prefix_stops_at_each_role(
        FailurePoint::ExistingOwnedVolume,
    ))
    .await?;
    Box::pin(assert_http_inspection_statuses_stop_first_storage_create()).await
}

async fn assert_storage_prefix_stops_at_each_role(point: FailurePoint) -> Result<(), String> {
    for (role, expected_prefix) in storage_role_prefixes() {
        Box::pin(assert_failed_storage_role(role, expected_prefix, point)).await?;
    }
    Ok(())
}

fn storage_role_prefixes() -> [(StorageRole, Vec<&'static str>); 5] {
    [
        (StorageRole::EngineMetadata, vec![]),
        (StorageRole::SharedRun, vec!["engine-metadata"]),
        (
            StorageRole::RunnerHome,
            vec!["engine-metadata", "shared-run"],
        ),
        (
            StorageRole::RunnerWork,
            vec!["engine-metadata", "shared-run", "runner-home"],
        ),
        (
            StorageRole::DindData,
            vec![
                "engine-metadata",
                "shared-run",
                "runner-home",
                "runner-work",
            ],
        ),
    ]
}

async fn assert_failed_storage_role(
    role: StorageRole,
    expected_prefix: Vec<&str>,
    point: FailurePoint,
) -> Result<(), String> {
    let failure = Box::pin(preparation_failure_for(role, point)).await?;
    assert_failure_error(&failure, role, point)?;
    let expected_volume = failure.expected_volume.clone();
    let captured = failure.finish().await?;
    assert_failure_response(&captured, role, expected_volume.as_deref(), point)?;
    assert_created_prefix(&captured, role, &expected_prefix)?;
    assert_no_container_create(&captured)
}

async fn preparation_failure_for(
    role: StorageRole,
    point: FailurePoint,
) -> Result<PreparationFailure, String> {
    let fixture = Fixture::open_with_engine("native-storage-claim-prefix", ENGINE_ID).await?;
    configure_capacity(&fixture).await?;
    let authority = crate::config::HostConfig::test_with_job_authority_policy(&fixture.binding)
        .map_err(|error| error.to_string())?;
    fixture
        .scope
        .install_job_authority_policy(&authority)
        .await
        .map_err(|error| error.to_string())?;
    let permit = StartPermit::test_fixture_with_engine_and_available_memory(
        &"a".repeat(32),
        ENGINE_ID,
        MEMORY_BYTES,
    );
    let images = super::super::super::super::support::test_images_for_engine(
        ENGINE_ID,
        permit.docker_root_digest(),
    );
    let engine = selected_engine(&images)?;
    let state = BootstrapAdmissionState::new(&fixture.binding, 1)?;
    let drive = bootstrap_drive(&fixture, &state.path);
    let poll = crate::launch_harness::available(&[REQUEST_ID]);
    let outcome = acquire_source_authorized(&drive, permit, &engine, &poll).await?;
    let (initializer_image, initializer) = initializer_for(&outcome, &fixture, &images).await?;
    let api = PairDockerApi::open_with_initializer(images.clone(), initializer_image, initializer)?;
    let expected_volume = inject_storage_failure(&fixture, &outcome, role, point).await?;
    let result = crate::launch::bootstrap::adapter::prepare_acquired(
        outcome,
        &api.docker,
        &fixture.scope,
        &engine,
        &images,
    )
    .await;
    let (error, ownership_error) = match result {
        Err(error) => (
            format!("Production storage preparation error: {error}"),
            matches!(error, crate::error::HostError::Ownership),
        ),
        Ok(None) => (
            "Production preparation returned no runner start.".to_owned(),
            false,
        ),
        Ok(Some(_)) => {
            return Err(format!(
                "The {} storage claim unexpectedly succeeded.",
                role.as_str()
            ));
        }
    };
    Ok(PreparationFailure {
        api,
        error: error.clone(),
        ownership_error,
        expected_volume,
    })
}

async fn inject_storage_failure(
    fixture: &Fixture,
    outcome: &crate::launch::steps::ScopedProvisioningOutcome,
    role: StorageRole,
    point: FailurePoint,
) -> Result<Option<String>, String> {
    match point {
        FailurePoint::JournalClaim => {
            install_claim_failure(&fixture.journal, role).await?;
            Ok(None)
        }
        FailurePoint::DockerInspect => {
            let reservation_id = acquired_reservation_id(outcome)?;
            let name = storage_volume_name(&fixture.journal, role, reservation_id).await?;
            CapturedRequest::inject_volume_inspect_transport_failure(&name);
            Ok(None)
        }
        FailurePoint::ExistingOwnedVolume => {
            let reservation_id = acquired_reservation_id(outcome)?;
            let (name, operation_id) =
                storage_volume_identity(&fixture.journal, role, reservation_id).await?;
            let labels = serde_json::json!({
                "velnor.connection": fixture.scope.connection_id().as_str(),
                "velnor.reservation": reservation_id.to_string(),
                "velnor.operation": operation_id,
                "velnor.role": role.as_str(),
            });
            CapturedRequest::inject_preexisting_owned_volume(&name, labels);
            Ok(Some(name))
        }
        FailurePoint::InspectStatus(status) => {
            let reservation_id = acquired_reservation_id(outcome)?;
            let name = storage_volume_name(&fixture.journal, role, reservation_id).await?;
            CapturedRequest::inject_volume_inspection_status(&name, status);
            Ok(Some(name))
        }
    }
}

async fn assert_http_inspection_statuses_stop_first_storage_create() -> Result<(), String> {
    for status in [401, 403, 500, 503] {
        let failure = Box::pin(preparation_failure_for(
            StorageRole::EngineMetadata,
            FailurePoint::InspectStatus(status),
        ))
        .await?;
        let expected_volume = failure
            .expected_volume
            .clone()
            .ok_or_else(|| "The HTTP failure case omitted its volume identity.".to_owned())?;
        let captured = failure.finish().await?;
        if !captured.requests.iter().any(|request| {
            request.method == "GET"
                && request.target.contains(&expected_volume)
                && request.response_status == Some(status)
        }) {
            return Err(format!(
                "The volume inspection did not return injected status {status}."
            ));
        }
        if captured.requests.iter().any(|request| {
            request.method == "POST"
                && (request.target == "/volumes/create"
                    || request.target.starts_with("/containers/create"))
        }) {
            return Err(format!(
                "HTTP {status} volume inspection allowed a later Docker effect."
            ));
        }
    }
    Ok(())
}

fn acquired_reservation_id(
    outcome: &crate::launch::steps::ScopedProvisioningOutcome,
) -> Result<i64, String> {
    match outcome {
        crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) => {
            Ok(acquired.reservation_id())
        }
        crate::launch::steps::ScopedProvisioningOutcome::Recovered(_)
        | crate::launch::steps::ScopedProvisioningOutcome::NoStart
        | crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(_) => {
            Err("The test outcome does not contain an acquired reservation.".to_owned())
        }
    }
}

async fn storage_volume_name(
    journal: &crate::journal::Journal,
    role: StorageRole,
    reservation_id: i64,
) -> Result<String, String> {
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    let mut rows = connection
        .query(
            "SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = ?2",
            (reservation_id, role.as_str()),
        )
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The selected storage role has no journaled name.".to_owned())?;
    row.get(0).map_err(|error| error.to_string())
}

async fn storage_volume_identity(
    journal: &crate::journal::Journal,
    role: StorageRole,
    reservation_id: i64,
) -> Result<(String, String), String> {
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    let mut rows = connection
        .query(
            "SELECT object.docker_name, reservation.intent_id FROM capacity_reservation_provisioning_objects AS object JOIN capacity_reservations AS reservation ON reservation.id = object.reservation_id WHERE object.reservation_id = ?1 AND object.role = ?2",
            (reservation_id, role.as_str()),
        )
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The selected storage role has no journaled identity.".to_owned())?;
    let name = row.get(0).map_err(|error| error.to_string())?;
    let operation_id: i64 = row.get(1).map_err(|error| error.to_string())?;
    Ok((name, operation_id.to_string()))
}

async fn install_claim_failure(
    journal: &crate::journal::Journal,
    role: StorageRole,
) -> Result<(), String> {
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            &format!(
                "CREATE TRIGGER reject_test_storage_claim BEFORE UPDATE OF create_state ON capacity_reservation_provisioning_objects WHEN OLD.role = '{}' AND NEW.create_state = 'requested' BEGIN SELECT RAISE(ABORT, 'injected storage claim failure'); END",
                role.as_str()
            ),
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}
