use std::collections::HashMap;

use serde_json::json;

use crate::journal::{CapacityLimits, Journal, StorageRole};
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole, test_resource_budget};

use crate::worker::ResourceBudgetConfig;

use super::{TMPFS_ROLES, create_and_initialize, initialization_limits};

#[path = "tests/prefix.rs"]
mod prefix;

#[cfg(unix)]
#[tokio::test]
async fn metadata_volume_uses_the_disk_role_and_records_exact_identity() -> Result<(), String> {
    let fixture = metadata_fixture().await?;
    let responses = metadata_responses(&fixture)?;
    let stub = crate::launch::inspect_tests::DockerStub::open(responses)?;
    let result = crate::launch::bootstrap::containers::create_metadata_storage_object(
        &stub.docker,
        &fixture.scope,
        fixture.reservation_id,
        &fixture.engine,
        &fixture.images,
        &prefix::observation(),
    )
    .await;
    let proof = result.map_err(|error| error.to_string())?;
    let requests = stub.finish().await?;
    assert_eq!(proof.reservation_id(), fixture.reservation_id);
    assert_eq!(proof.operation_id(), fixture.operation_id);
    assert_eq!(proof.role(), StorageRole::EngineMetadata);
    assert_eq!(proof.docker_name(), fixture.volume_name);
    assert_eq!(proof.docker_id(), fixture.volume_name);
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::EngineMetadata)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST ") && request.contains("/volumes/create"))
            .count(),
        1
    );
    Ok(())
}

struct MetadataFixture {
    _scratch: crate::journal::request_lifecycle::tests::helpers::Scratch,
    scope: crate::journal::ConnectionJournal,
    reservation_id: i64,
    operation_id: i64,
    images: crate::launch::VerifiedWorkerImages,
    engine: crate::launch::SelectedEngineContext,
    volume_name: String,
    connection_id: String,
}

async fn metadata_fixture() -> Result<MetadataFixture, String> {
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()
        .map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await
        .map_err(|error| error.to_string())?;
    let connection =
        crate::journal::request_lifecycle::tests::helpers::binding("metadata-producer", 1802)
            .map_err(|error| error.to_string())?;
    let scope = crate::journal::request_lifecycle::tests::helpers::created_scope(
        &journal,
        &connection,
        2802,
    )
    .await
    .map_err(|error| error.to_string())?;
    let reservation_id = prefix::prepare_reservation(&scope).await?;
    let (operation_id, _) = reservation_facts(&scratch.file(), reservation_id).await?;
    let names =
        WorkerIdentityNames::from_nonce(&"d".repeat(32)).map_err(|error| error.to_string())?;
    Ok(MetadataFixture {
        _scratch: scratch,
        scope,
        reservation_id,
        operation_id,
        images: prefix::images(),
        engine: prefix::selected_engine()?,
        volume_name: names.volume_name(WorkerVolumeRole::RunnerMetadata),
        connection_id: connection.connection_id.as_str().to_owned(),
    })
}

fn metadata_responses(
    fixture: &MetadataFixture,
) -> Result<Vec<crate::launch::inspect_tests::DockerResponse>, String> {
    let labels = HashMap::from([
        (
            "velnor.connection".to_owned(),
            fixture.connection_id.clone(),
        ),
        (
            "velnor.reservation".to_owned(),
            fixture.reservation_id.to_string(),
        ),
        (
            "velnor.operation".to_owned(),
            fixture.operation_id.to_string(),
        ),
        (
            "velnor.role".to_owned(),
            StorageRole::EngineMetadata.as_str().to_owned(),
        ),
    ]);
    let metadata = json!({
        "Name": fixture.volume_name,
        "Driver": "local",
        "Mountpoint": "/var/lib/docker/volumes/metadata/_data",
        "Labels": labels,
        "Scope": "local",
        "Options": {}
    })
    .to_string();
    let mut responses = Vec::new();
    append_verified_engine_responses(&mut responses, &fixture.images);
    append_verified_engine_responses(&mut responses, &fixture.images);
    responses.extend([
        crate::launch::inspect_tests::http(404, r#"{"message":"no such volume"}"#),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(201, &metadata),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &metadata),
        crate::launch::inspect_tests::http(200, &daemon_info()),
    ]);
    Ok(responses)
}

#[test]
fn initializer_uses_the_admission_reserve_and_fixed_role_order() {
    let budget = test_resource_budget().expect("test budget is valid");
    let limits = initialization_limits(budget).expect("initializer limits are valid");
    assert!(limits.cpu_nano() <= 1_000_000_000);
    assert!(limits.memory_bytes() <= 1_073_741_824 + 134_217_728);
    assert_eq!(limits.pids(), 16);
    assert_eq!(
        TMPFS_ROLES,
        [
            crate::journal::StorageRole::SharedRun,
            crate::journal::StorageRole::RunnerHome,
            crate::journal::StorageRole::RunnerWork,
            crate::journal::StorageRole::DindData,
        ]
    );
    let too_small = ResourceBudgetConfig {
        runner_cpu_millicores: 1_000,
        runner_memory_bytes: 6_291_456,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 6_291_456,
        operational_memory_reserve_bytes: 1,
    }
    .validate()
    .expect("valid pair budget");
    let bounded = initialization_limits(too_small).expect("small limits stay bounded");
    assert!(bounded.memory_bytes() <= 6_291_456);
}

async fn reservation_facts(
    path: &std::path::Path,
    reservation_id: i64,
) -> Result<(i64, (u64, u64)), String> {
    let path = path.to_str().ok_or("journal path is not UTF-8")?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = database.connect().map_err(|error| error.to_string())?;
    let mut intent_rows = connection
        .query(
            "SELECT intent_id FROM capacity_reservations WHERE id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|error| error.to_string())?;
    let intent_id = intent_rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("reservation row is absent")?
        .get::<i64>(0)
        .map_err(|error| error.to_string())?;
    let mut role_rows = connection
            .query(
                "SELECT charge_bytes, charge_inodes FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1 AND role = 'shared-run'",
                [reservation_id],
            )
            .await
            .map_err(|error| error.to_string())?;
    let role = role_rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("shared role row is absent")?;
    Ok((
        intent_id,
        (
            u64::try_from(role.get::<i64>(0).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?,
            u64::try_from(role.get::<i64>(1).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?,
        ),
    ))
}

fn append_verified_engine_responses(
    responses: &mut Vec<crate::launch::inspect_tests::DockerResponse>,
    images: &crate::launch::VerifiedWorkerImages,
) {
    let runner = image_inspect("velnor-runner", images, images.runner());
    let dind = image_inspect("velnor-dind", images, images.dind());
    responses.extend([
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &runner),
        crate::launch::inspect_tests::http(200, &dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
    ]);
}

fn daemon_info() -> String {
    json!({
        "ID": crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID,
        "DockerRootDir": "/var/lib/docker",
        "Architecture": "aarch64",
        "OSType": "linux"
    })
    .to_string()
}

fn image_inspect(
    name: &str,
    images: &crate::launch::VerifiedWorkerImages,
    image: &crate::launch::VerifiedImageIdentity,
) -> String {
    let reference = format!("{name}:v{}-{}", images.source_sha(), image.archive_sha256());
    json!({
        "Id": image.runtime_id(),
        "RepoTags": [reference],
        "RepoDigests": [],
        "Os": "linux",
        "Architecture": "amd64",
        "Descriptor": {
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "digest": image.image_index_digest().expect("verified index digest"),
            "size": image.image_index_size().expect("verified index size"),
            "annotations": {
                "io.containerd.image.name": format!("docker.io/library/{reference}"),
                "org.opencontainers.image.ref.name": reference.split_once(':').expect("tag").1
            }
        }
    })
    .to_string()
}

fn volume_json(name: &str, labels: &HashMap<String, String>, bytes: u64, inodes: u64) -> String {
    json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": "/var/lib/docker/volumes/owned/_data",
        "Labels": labels,
        "Scope": "local",
        "Options": {
            "type": "tmpfs",
            "device": "tmpfs",
            "o": format!("size={bytes},nr_inodes={inodes},nosuid,nodev")
        }
    })
    .to_string()
}
