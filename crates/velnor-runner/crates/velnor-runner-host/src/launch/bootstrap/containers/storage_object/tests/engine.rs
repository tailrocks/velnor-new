use super::*;

use super::body_stub::{DockerBodyStub, RecordedRequest};

#[cfg(unix)]
#[tokio::test]
async fn all_tmpfs_roles_use_their_claimed_finite_byte_and_inode_charges() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-all-tmpfs-roles").await?;
    let names = WorkerIdentityNames::from_nonce(NONCE).map_err(|error| error.to_string())?;
    let profile = fixture
        .scope
        .current_storage_profile()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "storage profile is missing".to_owned())?;
    let roles = [
        (
            StorageRole::SharedRun,
            crate::worker::WorkerVolumeRole::Socket,
        ),
        (
            StorageRole::RunnerHome,
            crate::worker::WorkerVolumeRole::Home,
        ),
        (
            StorageRole::RunnerWork,
            crate::worker::WorkerVolumeRole::Work,
        ),
        (
            StorageRole::DindData,
            crate::worker::WorkerVolumeRole::DindData,
        ),
    ];
    let mut responses = Vec::new();
    let mut expected = Vec::new();
    for (role, worker_role) in roles {
        let limit = profile
            .role_limit(role)
            .map_err(|error| error.to_string())?;
        let name = names.volume_name(worker_role);
        let labels = ownership_labels(&fixture, role);
        let volume =
            local_volume_with_limits(&name, &labels, limit.max_bytes(), limit.max_inodes());
        append_json_engine_responses(&mut responses, &fixture.images);
        append_json_engine_responses(&mut responses, &fixture.images);
        responses.extend([
            (404, r#"{"message":"no such volume"}"#.to_owned()),
            (200, daemon_info()),
            (201, volume.clone()),
            (200, daemon_info()),
            (200, volume),
            (200, daemon_info()),
        ]);
        expected.push((name, limit.max_bytes(), limit.max_inodes(), labels));
    }
    let stub = DockerBodyStub::open(responses)?;
    for (role, worker_role) in roles {
        let proof = create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            role,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .map_err(|error| error.to_string())?;
        assert_eq!(proof.docker_name(), names.volume_name(worker_role));
        assert_eq!(proof.role(), role);
    }
    let requests = stub.finish().await?;
    let creates = requests
        .iter()
        .filter(|request| {
            request.line.starts_with("POST ") && request.line.contains("/volumes/create")
        })
        .collect::<Vec<_>>();
    assert_eq!(creates.len(), roles.len());
    for (request, (name, bytes, inodes, labels)) in creates.into_iter().zip(expected) {
        assert_volume_create_request(request, &name, bytes, inodes, &labels)?;
    }
    Ok(())
}

fn assert_volume_create_request(
    request: &RecordedRequest,
    name: &str,
    bytes: u64,
    inodes: u64,
    labels: &HashMap<String, String>,
) -> Result<(), String> {
    let body: serde_json::Value =
        serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
    let options = tmpfs_options(bytes, inodes).map_err(|error| error.to_string())?;
    assert_eq!(body["Name"], name);
    assert_eq!(body["Driver"], "local");
    assert_eq!(body["DriverOpts"], serde_json::json!(options));
    assert_eq!(body["Labels"], serde_json::json!(labels));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn sample_expiry_during_precreate_verification_blocks_volume_inspect_and_create()
-> Result<(), String> {
    let fixture = physical_fixture("typed-volume-expiring-sample").await?;
    tokio::time::sleep(std::time::Duration::from_secs(25)).await;
    assert!(fixture.observation.is_fresh_for(
        ENGINE_ID,
        fixture.selected.docker_root_digest(),
        &"a".repeat(32)
    ));
    let runner = image_inspect("velnor-runner", &fixture.images, fixture.images.runner());
    let dind = image_inspect("velnor-dind", &fixture.images, fixture.images.dind());
    let mut responses = Vec::new();
    append_verified_engine_responses(&mut responses, &fixture.images);
    responses.extend([
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &runner),
        crate::launch::inspect_tests::http(200, &dind),
        crate::launch::inspect_tests::delayed_http(
            200,
            &daemon_info(),
            std::time::Duration::from_secs(6),
        ),
    ]);
    let stub = crate::launch::inspect_tests::DockerStub::open(responses)?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::RunnerHome,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .is_err()
    );
    let requests = stub.finish().await?;
    assert!(
        !requests.iter().any(|request| request.contains("/volumes/")),
        "expired sample reached a volume request: {requests:?}"
    );
    assert_eq!(
        object_create_state(&fixture, StorageRole::RunnerHome).await?,
        "uncertain"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn selected_engine_drift_before_create_blocks_post_and_holds_claim() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-engine-before-create").await?;
    let mut responses = Vec::new();
    append_verified_engine_responses(&mut responses, &fixture.images);
    append_verified_engine_responses(&mut responses, &fixture.images);
    responses.extend([
        crate::launch::inspect_tests::http(404, r#"{"message":"no such volume"}"#),
        crate::launch::inspect_tests::http(200, &daemon_info_for("changed-engine")),
    ]);
    let stub = crate::launch::inspect_tests::DockerStub::open(responses)?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::RunnerHome,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .is_err()
    );
    let requests = stub.finish().await?;
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/volumes/create")),
        "producer created a volume after selected-engine drift"
    );
    assert_eq!(
        object_create_state(&fixture, StorageRole::RunnerHome).await?,
        "uncertain"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn selected_engine_drift_after_create_blocks_inspect_receipt() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-engine-after-create").await?;
    let names = WorkerIdentityNames::from_nonce(NONCE).map_err(|error| error.to_string())?;
    let name = names.volume_name(crate::worker::WorkerVolumeRole::Home);
    let labels = ownership_labels(&fixture, StorageRole::RunnerHome);
    let volume = local_volume(&name, &labels);
    let mut responses = Vec::new();
    append_verified_engine_responses(&mut responses, &fixture.images);
    append_verified_engine_responses(&mut responses, &fixture.images);
    responses.extend([
        crate::launch::inspect_tests::http(404, r#"{"message":"no such volume"}"#),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(201, &volume),
        crate::launch::inspect_tests::http(200, &daemon_info_for("changed-engine")),
    ]);
    let stub = crate::launch::inspect_tests::DockerStub::open(responses)?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::RunnerHome,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .is_err()
    );
    let requests = stub.finish().await?;
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("POST ") && request.contains("/volumes/create")),
        "producer did not test the post-create engine boundary"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains(&format!("GET /volumes/{name} ")))
            .count(),
        1,
        "producer inspected the created object after engine identity changed"
    );
    assert_eq!(
        object_create_state(&fixture, StorageRole::RunnerHome).await?,
        "uncertain"
    );
    Ok(())
}
