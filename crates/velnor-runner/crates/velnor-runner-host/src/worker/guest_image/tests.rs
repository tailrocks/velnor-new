use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

use bollard::models::MountType;
use serde_json::Value;

use crate::config_test_support::{basic_connection, host_config};
use crate::launch::verified_images::{
    TestWorkerImageMutation, test_worker_images, test_worker_images_with,
};
use crate::worker::{
    bollard_create, dind_create_verified, join_dind_net, runner_create_verified,
    test_resource_budget,
};
use crate::{HostConfig, HostError, RepositoryConnection, runner_plan};

use super::write_guest_image;

#[test]
fn writer_publishes_canonical_read_only_guest_claims_only() -> Result<(), HostError> {
    let state = TestState::new()?;
    let connection = test_connection("build-main", "acme/project", "cache-account", "linux-build")?;
    let images = test_worker_images();
    let file = write_guest_image(&state.0, &connection, &images)?;
    let mount = file.runner_bind(&connection, &images)?;
    let bytes = fs::read(file.path()).map_err(|_| HostError::Path)?;
    let json: Value = serde_json::from_slice(&bytes).map_err(|_| HostError::ForbiddenMount)?;

    assert_eq!(mount.target, "/opt/velnor/guest-image.json");
    assert!(mount.read_only);
    assert!(
        mount
            .source
            .starts_with(state.0.to_str().ok_or(HostError::Path)?)
    );
    assert_eq!(json["schema"], "velnor.guest-image/v1");
    assert_eq!(json["source_sha"], images.source_sha());
    assert_eq!(json["guest_platform"], "linux/amd64");
    assert_eq!(json["scale_set"]["name"], connection.scale_set.name());
    let expected_labels = serde_json::to_value(connection.scale_set.labels())
        .map_err(|_| HostError::ForbiddenMount)?;
    assert_eq!(json["scale_set"]["labels"], expected_labels);
    assert_eq!(
        json["images"]["runner"]["archive_sha256"],
        images.runner().archive_sha256()
    );
    assert_eq!(
        json["images"]["runner"]["archive_index_digest"],
        images.runner().archive_index_digest()
    );
    assert_eq!(
        json["images"]["runner"]["image_index_digest"],
        images
            .runner()
            .image_index_digest()
            .ok_or(HostError::Docker)?
    );
    assert_eq!(
        json["images"]["runner"]["manifest_digest"],
        images.runner().manifest_digest()
    );
    assert_eq!(
        json["images"]["runner"]["config_digest"],
        images.runner().config_digest()
    );
    assert_eq!(
        json["images"]["dind"]["archive_sha256"],
        images.dind().archive_sha256()
    );
    for forbidden in [
        "credential",
        "token",
        "pat",
        "repository",
        "runtime_id",
        "engine_id",
        "docker_root_digest",
        "daemon_architecture",
    ] {
        assert!(!json.to_string().contains(forbidden));
    }
    let metadata = fs::symlink_metadata(file.path()).map_err(|_| HostError::Path)?;
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.mode() & 0o777, 0o444);
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    let repeated = write_guest_image(&state.0, &connection, &images)?;
    assert_eq!(file.path(), repeated.path());
    Ok(())
}

#[test]
fn verified_immutable_image_ids_and_metadata_reach_runner_docker_config() -> Result<(), HostError> {
    let state = TestState::new()?;
    let connection = test_connection("build-main", "acme/project", "cache-account", "linux-build")?;
    let images = test_worker_images();
    let guest_file = write_guest_image(&state.0, &connection, &images)?;
    let budget = test_resource_budget()?;
    let plan = runner_plan("worker_a")?;
    let runner = runner_create_verified(&plan, budget, &connection, &images, &guest_file)?;
    let runner = join_dind_net(runner, &"a".repeat(64))?;
    let dind = dind_create_verified("worker_a", budget, &images)?;
    let runner = bollard_create(&runner)?;
    let dind = bollard_create(&dind)?;

    assert_eq!(
        runner.config.image.as_deref(),
        Some(images.runner().runtime_id())
    );
    assert_eq!(
        dind.config.image.as_deref(),
        Some(images.dind().runtime_id())
    );
    assert_eq!(
        runner.config.env.as_deref(),
        Some(["DOCKER_HOST=tcp://127.0.0.1:2375".to_owned()].as_slice())
    );
    let host = runner
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    let mounts = host.mounts.as_ref().ok_or(HostError::Docker)?;
    let guest = mounts
        .iter()
        .find(|mount| mount.target.as_deref() == Some("/opt/velnor/guest-image.json"))
        .ok_or(HostError::Docker)?;
    assert_eq!(guest.typ, Some(MountType::BIND));
    assert_eq!(guest.source.as_deref(), guest_file.path().to_str());
    assert_eq!(guest.read_only, Some(true));
    assert!(mounts.iter().all(|mount| {
        mount.typ == Some(MountType::VOLUME)
            || mount.target.as_deref() == Some("/opt/velnor/guest-image.json")
    }));
    assert!(mounts.iter().all(|mount| {
        mount.source.as_deref().is_none_or(|source| {
            !source.contains("/Users/")
                && !source.contains("/home/")
                && !source.contains("docker.sock")
        })
    }));
    let dind_mounts = dind
        .config
        .host_config
        .as_ref()
        .and_then(|host| host.mounts.as_ref())
        .ok_or(HostError::Docker)?;
    assert!(
        dind_mounts
            .iter()
            .all(|mount| { mount.target.as_deref() != Some("/opt/velnor/guest-image.json") })
    );
    Ok(())
}

#[test]
fn malformed_verified_claims_and_changed_binding_fail_closed() -> Result<(), HostError> {
    let state = TestState::new()?;
    let connection = test_connection("build-main", "acme/project", "cache-account", "linux-build")?;
    for mutation in [
        TestWorkerImageMutation::RunnerRuntimeId,
        TestWorkerImageMutation::RunnerIndexDigest,
        TestWorkerImageMutation::RunnerManifestDigest,
        TestWorkerImageMutation::RunnerConfigDigest,
        TestWorkerImageMutation::Platform,
        TestWorkerImageMutation::SourceSha,
    ] {
        assert_eq!(
            write_guest_image(
                &state.0,
                &connection,
                &test_worker_images_with(Some(mutation))
            ),
            Err(HostError::ForbiddenMount)
        );
    }
    assert!(!state.0.join("guest-image").exists());

    let images = test_worker_images();
    let file = write_guest_image(&state.0, &connection, &images)?;
    let changed = test_connection("build-main", "acme/project", "cache-account", "linux-test")?;
    assert_eq!(
        file.runner_bind(&changed, &images),
        Err(HostError::ForbiddenMount)
    );
    assert!(file.runner_bind(&connection, &images).is_ok());
    Ok(())
}

#[test]
fn modified_guest_file_is_rejected_before_bind_projection() -> Result<(), HostError> {
    let state = TestState::new()?;
    let connection = test_connection("build-main", "acme/project", "cache-account", "linux-build")?;
    let images = test_worker_images();
    let file = write_guest_image(&state.0, &connection, &images)?;
    fs::set_permissions(file.path(), fs::Permissions::from_mode(0o644))
        .map_err(|_| HostError::Path)?;
    assert_eq!(
        file.runner_bind(&connection, &images),
        Err(HostError::ForbiddenMount)
    );
    Ok(())
}

fn test_connection(
    id: &str,
    repository: &str,
    account: &str,
    scale_set: &str,
) -> Result<RepositoryConnection, HostError> {
    let source = basic_connection(id, 42, repository, account, scale_set);
    let config = HostConfig::parse(&host_config(&[source], 1))?;
    config
        .connections()
        .first()
        .cloned()
        .ok_or(HostError::Config)
}

struct TestState(PathBuf);

impl TestState {
    fn new() -> Result<Self, HostError> {
        let path =
            std::env::temp_dir().join(format!("velnor-guest-image-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).map_err(|_| HostError::Path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        Ok(Self(path))
    }
}

impl Drop for TestState {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
