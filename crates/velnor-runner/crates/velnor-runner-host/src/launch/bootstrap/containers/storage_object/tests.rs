use std::collections::HashMap;

use bollard::models::{Volume, VolumeScopeEnum};
use serde_json::json;

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::journal::{
    AssignmentKey, CapacityLimits, ConnectionJournal, EngineContext, Journal, ReservationRequest,
    ReservationResult, StorageRole, fixture_provisioning_profile,
    fixture_storage_provisioning_admission,
};
use crate::launch::{SelectedEngineContext, StartPermit};
use crate::worker::WorkerIdentityNames;

use super::{create_storage_object, tmpfs_options, tmpfs_role, verify_volume};

const ENGINE_ID: &str = "selected-engine";
const NONCE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HOME_BYTES: u64 = 2_u64 << 30;
const HOME_INODES: u64 = 131_072;

struct Fixture {
    scratch: crate::launch_harness::Scratch,
    connection: RepositoryConnection,
    scope: ConnectionJournal,
    reservation_id: i64,
    intent_id: i64,
    selected: SelectedEngineContext,
    images: crate::launch::VerifiedWorkerImages,
    observation: crate::launch::StartObservationWitness,
}

async fn physical_fixture(label: &str) -> Result<Fixture, String> {
    let scratch = crate::launch_harness::Scratch::new(label).map_err(|error| error.to_string())?;
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
    journal
        .configure_storage_profile(fixture_provisioning_profile())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    let (connection, scope) = scoped_connection(&journal).await?;

    let permit = StartPermit::test_fixture_with_engine_id(&"a".repeat(32), ENGINE_ID);
    let observation = permit.observation_witness();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission =
        fixture_storage_provisioning_admission(&permit).map_err(|error| error.to_string())?;
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: NONCE.to_owned(),
        })
        .await
        .map_err(|error| error.to_string())?;
    let reservation = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await
        .map_err(|error| error.to_string())?;
    let ReservationResult::Reserved { reservation_id } = reservation else {
        return Err("typed reservation was not created".to_owned());
    };
    let (intent_id, fresh) = scope
        .begin_launch(&assignment)
        .await
        .map_err(|error| error.to_string())?;
    if fresh {
        return Err("typed launch intent was unexpectedly replaced".to_owned());
    }
    let images = crate::launch::verified_images::test_worker_images_for_root("/var/lib/docker");
    let engine_context = EngineContext {
        engine_id: ENGINE_ID.to_owned(),
        endpoint: "unix:///var/run/docker.sock".to_owned(),
        platform: "linux/amd64".to_owned(),
    };
    let selected =
        SelectedEngineContext::bind(&engine_context, &images).map_err(|error| error.to_string())?;
    Ok(Fixture {
        scratch,
        connection,
        scope,
        reservation_id,
        intent_id,
        selected,
        images,
        observation,
    })
}

async fn scoped_connection(
    journal: &Journal,
) -> Result<(RepositoryConnection, ConnectionJournal), String> {
    let connection = RepositoryConnection::try_new(
        ConnectionId::parse("object-producer").map_err(|error| error.to_string())?,
        701,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor/object-producer")
            .map_err(|error| error.to_string())?,
        ScaleSetSelector::try_new(
            "integration",
            &["velnor".to_owned(), "integration".to_owned()],
        )
        .map_err(|error| error.to_string())?,
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let scope = journal
        .connection_scope(&connection)
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_scale_set(1701, "integration", SetOwnership::Adopted)
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_engine_context(ENGINE_ID, "unix:///var/run/docker.sock", "linux/amd64")
        .await
        .map_err(|error| error.to_string())?;
    Ok((connection, scope))
}

fn daemon_info() -> String {
    daemon_info_for(ENGINE_ID)
}

fn image_inspect(
    name: &str,
    images: &crate::launch::VerifiedWorkerImages,
    identity: &crate::launch::VerifiedImageIdentity,
) -> String {
    let reference = format!(
        "{name}:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    let digest = identity.image_index_digest().expect("OCI index digest");
    let size = identity.image_index_size().expect("OCI index size");
    json!({
        "Id": identity.runtime_id(),
        "RepoTags": [reference],
        "RepoDigests": [],
        "Os": "linux",
        "Architecture": "amd64",
        "Descriptor": {
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "digest": digest,
            "size": size,
            "annotations": {
                "io.containerd.image.name": format!("docker.io/library/{reference}"),
                "org.opencontainers.image.ref.name": reference.split_once(':').expect("image tag").1
            }
        }
    })
    .to_string()
}

fn local_volume(name: &str, labels: &HashMap<String, String>) -> String {
    local_volume_with_limits(name, labels, HOME_BYTES, HOME_INODES)
}

fn local_volume_with_limits(
    name: &str,
    labels: &HashMap<String, String>,
    bytes: u64,
    inodes: u64,
) -> String {
    json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": "/var/lib/docker/volumes/owned-volume/_data",
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

fn ownership_labels(fixture: &Fixture, role: StorageRole) -> HashMap<String, String> {
    HashMap::from([
        ("velnor.connection".to_owned(), "object-producer".to_owned()),
        (
            "velnor.reservation".to_owned(),
            fixture.reservation_id.to_string(),
        ),
        ("velnor.operation".to_owned(), fixture.intent_id.to_string()),
        ("velnor.role".to_owned(), role.as_str().to_owned()),
    ])
}

#[path = "tests/body_stub.rs"]
mod body_stub;
#[path = "tests/engine.rs"]
mod engine;
#[path = "tests/integration.rs"]
mod integration;
#[path = "tests/validation.rs"]
mod validation;

fn append_json_engine_responses(
    responses: &mut Vec<(u16, String)>,
    images: &crate::launch::VerifiedWorkerImages,
) {
    let runner = image_inspect("velnor-runner", images, images.runner());
    let dind = image_inspect("velnor-dind", images, images.dind());
    responses.extend([
        (200, daemon_info()),
        (200, runner),
        (200, dind),
        (200, daemon_info()),
    ]);
}

fn daemon_info_for(engine_id: &str) -> String {
    json!({
        "ID": engine_id,
        "DockerRootDir": "/var/lib/docker",
        "Architecture": "aarch64",
        "OSType": "linux"
    })
    .to_string()
}

async fn object_create_state(fixture: &Fixture, role: StorageRole) -> Result<String, String> {
    let path = fixture.scratch.file();
    let database = turso::Builder::new_local(path.to_str().ok_or("journal path is not UTF-8")?)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let connection = database.connect().map_err(|error| error.to_string())?;
    let mut rows = connection
        .query(
            "SELECT create_state FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = ?2",
            (fixture.reservation_id, role.as_str()),
        )
        .await
        .map_err(|error| error.to_string())?;
    rows.next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "storage object row is missing".to_owned())?
        .get(0)
        .map_err(|error| error.to_string())
}
