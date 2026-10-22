use std::path::{Path, PathBuf};

use super::request_checks::verify_pair_requests;
use super::*;

#[path = "bootstrap_start/preflight.rs"]
mod preflight;
use preflight::assert_verified_preflight_before_mutations;
#[path = "bootstrap_start/identity_boundary.rs"]
mod identity_boundary;
#[path = "bootstrap_start/native_intake.rs"]
mod native_intake;
pub(super) use native_intake::{
    NATIVE_COMPOSITION_JOB_ID, PollSession, open_composition_with_poll, replay_owned_terminal_offer,
};
#[path = "bootstrap_start/partial_failure.rs"]
mod partial_failure;
pub(crate) use partial_failure::{
    RunnerCreateFailure, runner_create_409, runner_create_409_with_jobs,
};

const ENGINE_ID: &str = "bc9058a0-c807-412b-a088-6c1d96ddd462";
const MEMORY_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const INITIALIZER_ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct BootstrapAdmissionState {
    path: PathBuf,
}

impl BootstrapAdmissionState {
    fn new(binding: &crate::config::RepositoryConnection, jobs: u32) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "velnor-bootstrap-admission-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).map_err(|error| error.to_string())?;
        let connection = toml::to_string(binding).map_err(|error| error.to_string())?;
        let connection = connection
            .replace("[scale_set]", "[connections.scale_set]")
            .replace("[source_policy]", "[connections.source_policy]");
        let config = format!(
            "schema = 2\n[[connections]]\n{connection}\n[host]\nmax_jobs = {jobs}\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 4294967296\ndind_cpu_millicores = 1000\ndind_memory_bytes = 4294967296\noperational_memory_reserve_bytes = 1\n[docker]\ncontext = \"orbstack\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///var/run/docker.sock\"\n"
        );
        std::fs::write(path.join("host.toml"), config).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }
}

impl Drop for BootstrapAdmissionState {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

fn bootstrap_drive(fixture: &Fixture, path: &Path) -> crate::launch::Drive {
    crate::launch::Drive {
        set_id: SET_ID,
        queue_path: "https://queue.example.invalid/".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
        docker_engine_id: Some(ENGINE_ID.to_owned()),
        owner: "owner".to_owned(),
        repo: "repository".to_owned(),
        pat: "test-token".to_owned(),
        connection: Some(fixture.binding.clone()),
        authority_policy: Some(fixture.authority_policy.clone()),
        host_journal: Some(fixture.journal.clone()),
        scoped_journal: Some(fixture.scope.clone()),
        allocated_header: 1,
        source_authorized: true,
        admission_state: Some(path.to_path_buf()),
    }
}

#[test]
fn production_bootstrap_starts_private_pair_and_keeps_jit_withheld() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "production-bootstrap-private-pair",
        || Box::pin(production_bootstrap_starts_private_pair_and_keeps_jit_withheld_inner()),
    )
}

async fn production_bootstrap_starts_private_pair_and_keeps_jit_withheld_inner()
-> Result<(), String> {
    let setup = BootstrapSetup::open().await?;
    let reservation_id = setup.prepared.acquired().reservation_id();
    let operation_id = setup.prepared.acquired().intent_id();
    let request = setup.prepared.acquired().request().clone();
    let budget = setup.prepared.acquired().resource_budget();
    let network_name = setup.network.network_name().to_owned();
    let network_id = setup.network.network_id().to_owned();
    let images = setup.images.clone();
    let started = match setup.start().await {
        Ok(started) => started,
        Err(error) => {
            let evidence = setup.api.finish().await?;
            let routes = evidence
                .requests
                .iter()
                .map(|request| request.target.as_str())
                .collect::<Vec<_>>();
            return Err(format!(
                "{error}; fixture error: {:?}; routes: {routes:?}",
                evidence.server_error
            ));
        }
    };

    assert_eq!(started.runner_container_id(), RUNNER_ID);
    assert_eq!(started.dind_container_id(), DIND_ID);
    assert!(started.runner_stdin_open());
    assert!(started.jit_withheld());
    assert_eq!(started.receipt().reservation_id(), reservation_id);
    assert_eq!(started.receipt().operation_id(), operation_id);
    assert_eq!(started.receipt().engine_id(), ENGINE_ID);
    assert_eq!(started.mount_handoff().runner_container_id(), RUNNER_ID);
    assert_eq!(started.mount_handoff().dind_container_id(), DIND_ID);
    assert_eq!(started.mount_handoff().reservation_id(), reservation_id);
    assert_eq!(started.mount_handoff().operation_id(), operation_id);

    drop(started);
    let BootstrapSetup {
        fixture,
        api,
        prepared,
        ..
    } = setup;
    drop(prepared);
    let captured = api.finish().await?;
    assert!(
        captured.server_error.is_none(),
        "{:?}",
        captured.server_error
    );
    assert_eq!(captured.jit_input, [] as [u8; 0]);
    assert_no_storage_observation(&captured.requests)?;
    verify_pair_requests(&captured.requests, &network_name, budget, false)?;
    assert_verified_preflight_before_mutations(&captured.requests, &images)?;
    assert_eq!(
        network_id,
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    );
    verify_retained_charge(&fixture, reservation_id, operation_id, &request).await
}

pub(super) struct BootstrapSetup {
    fixture: Fixture,
    api: PairDockerApi,
    prepared: crate::launch::bootstrap::PreparedAcquiredBootstrap,
    network: crate::journal::RecordedStorageNetwork,
    engine: crate::launch::SelectedEngineContext,
    images: crate::launch::VerifiedWorkerImages,
}

impl BootstrapSetup {
    async fn start(&self) -> Result<crate::launch::bootstrap::StartedBootstrapPair, String> {
        let pending = self
            .prepared
            .storage()
            .pending_initialization()
            .ok_or_else(|| {
                "The storage initializer is not ready for the mount handoff.".to_owned()
            })?;
        crate::launch::bootstrap::start_bootstrap_pair(
            crate::launch::bootstrap::BootstrapPairInput {
                prepared: &self.prepared,
                network: &self.network,
                metadata: pending.metadata_handoff(),
                pending_initialization: pending,
                docker: &self.api.docker,
                journal: &self.fixture.scope,
                engine: &self.engine,
                images: &self.images,
            },
        )
        .await
        .map_err(|failure| {
            let (error, _) = failure.into_parts();
            format!("Production bootstrap pair start error: {error}")
        })
    }
}

async fn configure_capacity(fixture: &Fixture) -> Result<(), String> {
    configure_capacity_limits(fixture, 1).await?;
    fixture
        .journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await
        .map_err(|error| format!("Storage profile setup error: {error}"))
}

async fn configure_capacity_for_jobs(fixture: &Fixture, jobs: u32) -> Result<(), String> {
    configure_capacity_limits(fixture, jobs).await
}

async fn configure_capacity_limits(fixture: &Fixture, jobs: u32) -> Result<(), String> {
    let job_count = u64::from(jobs);
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            crate::journal::CapacityLimits {
                jobs,
                cpu_millicores: 4_000 * job_count,
                memory_bytes: (MEMORY_BYTES + (32_u64 << 30)).saturating_mul(job_count),
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 256 * 1024 * 1024,
            },
        )
        .await
        .map_err(|error| format!("Capacity setup error: {error}"))?;
    Ok(())
}

async fn acquire_source_authorized(
    drive: &crate::launch::Drive,
    permit: StartPermit,
    engine: &crate::launch::SelectedEngineContext,
    poll: &velnor_runner_github::Poll,
) -> Result<crate::launch::steps::ScopedProvisioningOutcome, String> {
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    let outcome = crate::launch::steps::acquire_scoped(&mut lane, drive, poll, permit, engine)
        .await
        .map_err(|error| format!("Source-authorized Acquire error: {error}"))?;
    assert_eq!(lane.paths.len(), 1);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    Ok(outcome)
}

async fn initializer_for(
    outcome: &crate::launch::steps::ScopedProvisioningOutcome,
    fixture: &Fixture,
    images: &crate::launch::VerifiedWorkerImages,
) -> Result<
    (
        String,
        crate::worker::storage_initialization::InitializationTestResponder,
    ),
    String,
> {
    let acquired = match outcome {
        crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) => acquired,
        crate::launch::steps::ScopedProvisioningOutcome::Recovered(_) => {
            return Err("The Acquire result is recovery.".to_owned());
        }
        crate::launch::steps::ScopedProvisioningOutcome::NoStart => {
            return Err("The Acquire result has no start.".to_owned());
        }
        crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(_) => {
            return Err("The Acquire result proves no start.".to_owned());
        }
    };
    let nonce = acquired
        .runner_name()
        .strip_prefix('v')
        .ok_or_else(|| "Acquired runner identity has no prefix.".to_owned())?;
    let names =
        crate::worker::WorkerIdentityNames::from_nonce(nonce).map_err(|error| error.to_string())?;
    let helper_image =
        crate::worker::storage_initialization::StorageInitializationImage::from_verified_release(
            images,
        )
        .map_err(|error| format!("The verified storage initializer is missing. Error: {error}."))?;
    let metadata = format!("{}-metadata", names.private_volume());
    let responder = crate::worker::storage_initialization::InitializationTestResponder::new(
        &fixture.scope,
        acquired.reservation_id(),
        &names.volume_name(crate::worker::WorkerVolumeRole::Home),
        &names.volume_name(crate::worker::WorkerVolumeRole::Work),
        &metadata,
        helper_image.runtime_id(),
        ENGINE_ID,
    )
    .await?;
    Ok((helper_image.runtime_id().to_owned(), responder))
}

async fn prepare_acquired(
    outcome: crate::launch::steps::ScopedProvisioningOutcome,
    fixture: &Fixture,
    api: &PairDockerApi,
    engine: &crate::launch::SelectedEngineContext,
    images: &crate::launch::VerifiedWorkerImages,
) -> Result<crate::launch::bootstrap::PreparedAcquiredBootstrap, String> {
    crate::launch::bootstrap::adapter::prepare_acquired(
        outcome,
        &api.docker,
        &fixture.scope,
        engine,
        images,
    )
    .await
    .map_err(|error| format!("Production storage preparation error: {error}"))?
    .ok_or_else(|| "The production storage preparation result has no start.".to_owned())
}

pub(super) struct CompositionSetup {
    pub(super) fixture: Fixture,
    pub(super) api: PairDockerApi,
    pub(super) outcome: Option<crate::launch::steps::ScopedProvisioningOutcome>,
    pub(super) engine: crate::launch::SelectedEngineContext,
    pub(super) images: crate::launch::VerifiedWorkerImages,
    pub(super) guest_image: crate::worker::guest_image::test_support::TestGuestImage,
    pub(super) poll_session: Option<native_intake::PollSession>,
    state: BootstrapAdmissionState,
}

impl CompositionSetup {
    pub(super) fn admission_state_path(&self) -> &Path {
        &self.state.path
    }
}

pub(super) use native_intake::authorize_actual_started_request;

pub(super) async fn open_composition() -> Result<CompositionSetup, String> {
    let poll = crate::launch_harness::available(&[REQUEST_ID]);
    Box::pin(open_composition_with_poll(&poll, 0)).await
}

async fn verify_retained_charge(
    fixture: &Fixture,
    reservation_id: i64,
    operation_id: i64,
    request: &crate::journal::AssignmentIdentity,
) -> Result<(), String> {
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or_else(|| "The durable reservation charge is absent.".to_owned())?;
    assert_eq!(row.intent_id, operation_id);
    assert_eq!(
        row.reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Absent
    );
    assert_eq!(row.acquired_request.as_ref(), Some(request));
    assert_eq!(row.runner_container_id.as_deref(), Some(RUNNER_ID));
    assert_eq!(row.dind_container_id.as_deref(), Some(DIND_ID));
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    Ok(())
}

fn assert_no_storage_observation(requests: &[CapturedRequest]) -> Result<(), String> {
    if requests
        .iter()
        .any(|request| route(request).contains("/exec"))
    {
        return Err(
            "Docker storage observation started before the pair-start checkpoint.".to_owned(),
        );
    }
    Ok(())
}
