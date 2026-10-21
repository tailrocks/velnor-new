use std::time::Instant;

use crate::{
    config::RepositoryConnection,
    controller_ipc::ControlState,
    error::HostError,
    journal::{ProbeSeed, ResourceProbePurpose, StorageCleanupIdentity},
    launch::SelectedEngineContext,
};

use super::support::{
    InventoryDocker, config_and_engine, inventory_docker, inventory_docker_on_thread,
};

pub(super) struct BusyHostFixture {
    pub(super) fixture: crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    pub(super) binding: RepositoryConnection,
    pub(super) selected: SelectedEngineContext,
    pub(super) state: std::path::PathBuf,
    pub(super) control: ControlState,
    pub(super) identity: StorageCleanupIdentity,
    pub(super) docker: InventoryDocker,
}

pub(super) async fn prepare_busy_host() -> Result<BusyHostFixture, String> {
    prepare_busy_host_with_sample(true, false).await
}

pub(super) async fn prepare_busy_host_without_sample_on_thread() -> Result<BusyHostFixture, String>
{
    prepare_busy_host_with_sample(false, true).await
}

pub(super) fn poll_receipt(
    host: &BusyHostFixture,
) -> Result<crate::controller_ipc::SessionPollReceipt, String> {
    host.control
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .and_then(|receipts| receipts.into_iter().next())
        .ok_or_else(|| "The controller has no completed poll receipt.".to_owned())
}

async fn prepare_busy_host_with_sample(
    collect_sample: bool,
    docker_on_thread: bool,
) -> Result<BusyHostFixture, String> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture()
        .await
        .map_err(|error| format!("The test fixture could not be created. Error: {error}."))?;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)
            .map_err(|error| {
                format!("The test could not create the connection binding. Error: {error}.")
            })?;
    let (config, selected, state) =
        config_and_engine(&fixture.scope, &binding, &fixture.database_path).await?;
    let sample = if collect_sample {
        Some(
            Box::pin(crate::launch::collect_sample_for_engine(
                &fixture.journal,
                selected.engine_id(),
            ))
            .await
            .map_err(|error| {
                format!("The test could not collect the capacity sample. Error: {error}.")
            })?,
        )
    } else {
        None
    };
    let control = ControlState::new(&config);
    control.set_admission_ready(true);
    let session_intent_id = record_observed_busy_session(&fixture.scope).await?;
    if let Some(sample) = &sample {
        control
            .cache_readiness_sample(sample.clone())
            .map_err(|error| {
                format!("The test could not cache the capacity sample. Error: {error}.")
            })?;
        validate_busy_sample(&fixture, sample, &selected).await?;
    }
    control
        .record_session_poll(&binding, 501, session_intent_id, "session-busy")
        .map_err(|error| format!("The test could not record the poll receipt. Error: {error}."))?;
    assert_physical_owner_without_request_key(&fixture).await?;
    let identity = fixture
        .scope
        .storage_cleanup_identity(fixture.reservation_id)
        .await
        .map_err(|error| format!("The test could not read the cleanup identity. Error: {error}."))?
        .ok_or_else(|| "The started reservation has no cleanup identity.".to_owned())?;
    let docker = if docker_on_thread {
        inventory_docker_on_thread(&binding, &identity, selected.engine_id())
    } else {
        inventory_docker(&binding, &identity, selected.engine_id())
    }
    .map_err(|error| format!("The test could not prepare the Docker inventory. Error: {error}."))?;
    Ok(BusyHostFixture {
        fixture,
        binding,
        selected,
        state,
        control,
        identity,
        docker,
    })
}

async fn record_observed_busy_session(
    scope: &crate::journal::ConnectionJournal,
) -> Result<i64, String> {
    let prepared = scope
        .prepare_session_create("dddddddddddddddddddddddddddddddd")
        .await
        .map_err(|error| {
            format!("The test could not record the session intent. Error: {error}.")
        })?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| format!("The test could not claim the session request. Error: {error}."))?
        .ok_or_else(|| "The durable session request was not claimable.".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-busy", 1)
        .await
        .map_err(|error| {
            format!("The test could not record the session binding. Error: {error}.")
        })?;
    let attempt = scope
        .begin_session_poll(bound.intent_id(), bound.allocation())
        .await
        .map_err(|error| {
            format!("The test could not begin the initial header poll. Error: {error}.")
        })?;
    let proof = crate::launch::CompletedHeaderPoll::after_test_response(
        attempt,
        bound.allocation(),
        scope,
        501,
        "session-busy",
    )
    .map_err(|error| {
        format!("The test could not create the initial header poll proof. Error: {error}.")
    })?;
    scope
        .settle_session_poll(bound.intent_id(), proof)
        .await
        .map_err(|error| {
            format!("The test could not settle the initial header poll. Error: {error}.")
        })?;
    Ok(bound.intent_id())
}

async fn assert_physical_owner_without_request_key(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
) -> Result<(), String> {
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| {
            format!("The test could not read the physical inventory. Error: {error}.")
        })?;
    if rows.iter().any(|row| {
        row.reservation_id == fixture.reservation_id
            && row
                .ownership
                .assignment()
                .is_some_and(|assignment| assignment.request_id().is_none())
    }) {
        Ok(())
    } else {
        Err(
            "The fixture must retain a known physical owner without request-key identity."
                .to_owned(),
        )
    }
}

async fn validate_busy_sample(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    sample: &crate::launch::ReadinessSample,
    selected: &SelectedEngineContext,
) -> Result<(), String> {
    if !sample.is_recent(Instant::now()) || !sample.matches_engine(selected) {
        return Err(format!(
            "The collected sample is stale or belongs to another engine: observed={:?}, generation={:?}, engine={:?}.",
            sample.observed_at(),
            sample.generation(),
            selected.engine_id()
        ));
    }
    let capacity = fixture
        .journal
        .readiness_capacity_usage()
        .await
        .map_err(|error| {
            format!("The test could not read the capacity snapshot. Error: {error}.")
        })?;
    if capacity.valid_for_guest(
        sample.effective_cpu_millicores(),
        sample.memory_total_bytes(),
        sample.memory_available_bytes(),
    ) {
        Ok(())
    } else {
        Err(format!(
            "The measured busy-host capacity does not fit: capacity={capacity:?}, cpu={}, total={}, available={}.",
            sample.effective_cpu_millicores(),
            sample.memory_total_bytes(),
            sample.memory_available_bytes()
        ))
    }
}

pub(super) async fn assert_current_ready(
    journal: &crate::journal::Journal,
    state: &std::path::Path,
    selected: &SelectedEngineContext,
    control: &ControlState,
    revision_before_refresh: u64,
) -> Result<(), String> {
    let (config, digest) = crate::readiness_scan::controller_config(state)
        .ok_or_else(|| "The controller test configuration could not be read.".to_owned())?;
    let revision = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    if revision != revision_before_refresh {
        return Err(format!(
            "The readiness audit changed the journal revision from {revision_before_refresh} to {revision}."
        ));
    }
    let instance = journal
        .instance_id()
        .await
        .map_err(|error| error.to_string())?;
    if control
        .readiness_matches(
            &digest,
            selected.engine_id(),
            selected.docker_root_digest(),
            &instance,
            revision,
            Instant::now(),
        )
        .map_err(|error| error.to_string())?
    {
        Ok(())
    } else {
        Err(format!(
            "The controller did not publish Ready for configuration {config:?}."
        ))
    }
}

pub(super) async fn probe_seed(
    journal: &crate::journal::Journal,
    selected: &SelectedEngineContext,
) -> Result<ProbeSeed, HostError> {
    let operation_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_owned();
    Ok(ProbeSeed {
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        instance_id: journal.instance_id().await?,
        engine_id: selected.engine_id().to_owned(),
        docker_root_digest: selected.docker_root_digest().to_owned(),
        operation_id,
        source_revision: "b7c3309d33451f8e256a7e255239e9dc9285f042".to_owned(),
        runtime_image_id: format!("sha256:{}", "a".repeat(64)),
        image_binding_digest: "c".repeat(64),
        projection_digest: "d".repeat(64),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    })
}
