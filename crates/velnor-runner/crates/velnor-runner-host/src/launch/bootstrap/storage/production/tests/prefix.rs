use std::collections::HashMap;

use super::*;
use crate::journal::{
    AssignmentKey, CapacityLimits, EngineContext, Journal, ReservationRequest, ReservationResult,
    StorageAdmissionSample, StorageFilesystemKind, StorageMedium, StoragePoolObservation,
    StorageProfile, StorageProvisioningAdmission, StorageProvisioningMemoryObservation,
    StorageRole, StorageSampleIdentity, fixture_provisioning_profile,
};
use crate::launch::{SelectedEngineContext, StartPermit};
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

struct PrefixFixture {
    scratch: crate::journal::request_lifecycle::tests::helpers::Scratch,
    scope: crate::journal::ConnectionJournal,
    reservation_id: i64,
    stub: crate::launch::inspect_tests::DockerStub,
}

#[cfg(unix)]
#[tokio::test]
async fn foreign_second_role_holds_prefix_and_blocks_later_effects() -> Result<(), String> {
    let fixture = prefix_fixture().await?;
    let (intent_id, _) = reservation_facts(&fixture.scratch.file(), fixture.reservation_id)
        .await
        .map_err(|error| format!("metadata receipt facts: {error}"))?;
    let names =
        WorkerIdentityNames::from_nonce(&"d".repeat(32)).map_err(|error| error.to_string())?;
    let worker_images = images();
    let metadata = crate::launch::bootstrap::containers::test_created_storage_object(
        fixture.reservation_id,
        intent_id,
        crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID.to_owned(),
        worker_images.docker_root_digest().to_owned(),
        StorageRole::EngineMetadata,
        names.volume_name(WorkerVolumeRole::RunnerMetadata),
        names.volume_name(WorkerVolumeRole::RunnerMetadata),
    )
    .map_err(|error| format!("metadata object fixture: {error}"))?;
    let engine = selected_engine()?;
    let images = images();
    let observation = observation();
    let context = crate::launch::bootstrap::adapter::StorageCreationContext {
        docker: &fixture.stub.docker,
        journal: &fixture.scope,
        engine: &engine,
        images: &images,
        observation: &observation,
        metadata: &metadata,
    };
    assert!(
        super::super::create_and_initialize(
            &context,
            fixture.reservation_id,
            intent_id,
            test_resource_budget().map_err(|error| error.to_string())?,
        )
        .await
        .is_err()
    );
    let requests = fixture.stub.finish().await?;
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST ") && request.contains("/volumes/create"))
            .count(),
        1
    );
    assert!(
        !requests.iter().any(|request| {
            request.starts_with("POST ") && request.contains("/containers/create")
        }),
        "initializer ran after the second role failed: {requests:?}"
    );
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::SharedRun)
            .await
            .map_err(|error| format!("shared role claim: {error}"))?
            .is_none()
    );
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::RunnerHome)
            .await
            .map_err(|error| format!("home role claim: {error}"))?
            .is_none()
    );
    Ok(())
}

async fn prefix_fixture() -> Result<PrefixFixture, String> {
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()
        .map_err(|error| format!("scratch: {error}"))?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| format!("journal open: {error}"))?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await
        .map_err(|error| format!("capacity setup: {error}"))?;
    let connection =
        crate::journal::request_lifecycle::tests::helpers::binding("prefix-producer", 1801)
            .map_err(|error| format!("connection binding: {error}"))?;
    let scope = crate::journal::request_lifecycle::tests::helpers::created_scope(
        &journal,
        &connection,
        2801,
    )
    .await
    .map_err(|error| format!("connection scope: {error}"))?;
    let reservation_id = prepare_reservation(&scope).await?;
    let (intent_id, limits) = reservation_facts(&scratch.file(), reservation_id)
        .await
        .map_err(|error| format!("reservation facts: {error}"))?;
    let names =
        WorkerIdentityNames::from_nonce(&"d".repeat(32)).map_err(|error| error.to_string())?;
    let shared_labels = HashMap::from([
        (
            "velnor.connection".to_owned(),
            connection.connection_id.as_str().to_owned(),
        ),
        ("velnor.reservation".to_owned(), reservation_id.to_string()),
        ("velnor.operation".to_owned(), intent_id.to_string()),
        (
            "velnor.role".to_owned(),
            StorageRole::SharedRun.as_str().to_owned(),
        ),
    ]);
    let shared = volume_json(
        &names.volume_name(WorkerVolumeRole::Socket),
        &shared_labels,
        limits.0,
        limits.1,
    );
    let foreign_home = volume_json(
        &names.volume_name(WorkerVolumeRole::Home),
        &HashMap::from([("owner".to_owned(), "other-job".to_owned())]),
        1,
        1,
    );
    let stub = crate::launch::inspect_tests::DockerStub::open(responses(
        &images(),
        &shared,
        &foreign_home,
    ))
    .map_err(|error| format!("docker stub: {error}"))?;
    Ok(PrefixFixture {
        scratch,
        scope,
        reservation_id,
        stub,
    })
}

pub(super) async fn prepare_reservation(
    scope: &crate::journal::ConnectionJournal,
) -> Result<i64, String> {
    const JOB_MEMORY: u64 = 8_589_934_592;
    let permit = StartPermit::test_fixture_with_engine_id(
        &"d".repeat(32),
        crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID,
    );
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "d".repeat(32),
        })
        .await
        .map_err(|error| error.to_string())?;
    let profile = fixture_provisioning_profile();
    let engine_id = permit.engine_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let pools = disk_pool_observations(&profile)?;
    let sample = StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            sample_generation_id.clone(),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            engine_id.clone(),
            docker_root_digest.clone(),
        )
        .map_err(|error| format!("sample identity: {error}"))?,
        std::time::Instant::now(),
        pools,
        None,
    )
    .map_err(|error| format!("sample validation: {error}"))?;
    let admission = StorageProvisioningAdmission::new(
        sample,
        StorageProvisioningMemoryObservation::new(64_u64 << 30, 32_u64 << 30)
            .map_err(|error| format!("provisioning memory sample: {error}"))?,
    );
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: &engine_id,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await
        .map_err(|error| format!("reservation: {error}"))?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err("valid storage proof did not reserve capacity".to_owned());
    };
    let (_, fresh) = scope
        .begin_launch(&assignment)
        .await
        .map_err(|error| format!("begin launch: {error}"))?;
    if fresh {
        return Err("reservation did not have an acquired launch intent".to_owned());
    }
    Ok(reservation_id)
}

fn disk_pool_observations(profile: &StorageProfile) -> Result<Vec<StoragePoolObservation>, String> {
    profile
        .pools()
        .iter()
        .filter(|pool| pool.medium() == StorageMedium::Disk)
        .map(|pool| {
            StoragePoolObservation::new_with_filesystem(
                pool.pool_id().to_owned(),
                pool.pool_id().to_owned(),
                pool.medium(),
                StorageFilesystemKind::Btrfs,
                pool.max_bytes(),
                pool.max_bytes(),
                pool.max_inodes(),
                pool.max_inodes(),
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("pool sample: {error}"))
}

fn responses(
    images: &crate::launch::VerifiedWorkerImages,
    shared: &str,
    foreign_home: &str,
) -> Vec<crate::launch::inspect_tests::DockerResponse> {
    let mut output = Vec::new();
    append_verified_engine_responses(&mut output, images);
    append_verified_engine_responses(&mut output, images);
    output.extend([
        crate::launch::inspect_tests::http(404, r#"{"message":"no such volume"}"#),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(201, shared),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, shared),
        crate::launch::inspect_tests::http(200, &daemon_info()),
    ]);
    append_verified_engine_responses(&mut output, images);
    append_verified_engine_responses(&mut output, images);
    output.push(crate::launch::inspect_tests::http(200, foreign_home));
    output
}

pub(super) fn images() -> crate::launch::VerifiedWorkerImages {
    let root_digest = crate::launch::resource_probe::docker_root_digest("/var/lib/docker")
        .expect("test Docker root is valid");
    crate::launch::verified_images::test_worker_images_for(
        crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID,
        &root_digest,
        "aarch64",
        "linux/amd64",
    )
}

pub(super) fn observation() -> crate::launch::StartObservationWitness {
    StartPermit::test_fixture_with_engine_id(
        &"d".repeat(32),
        crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID,
    )
    .observation_witness()
}

pub(super) fn selected_engine() -> Result<SelectedEngineContext, String> {
    SelectedEngineContext::bind(
        &EngineContext {
            engine_id: crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID.to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: "linux/amd64".to_owned(),
        },
        &images(),
    )
    .map_err(|error| error.to_string())
}
