use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

use serde_json::Value;

use crate::config_test_support::{basic_connection, host_config};
use crate::launch::verified_images::{
    TestWorkerImageMutation, test_worker_images, test_worker_images_with,
};
use crate::{HostConfig, HostError, RepositoryConnection};

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
fn verified_immutable_image_ids_and_metadata_reach_bootstrap_docker_config() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "verified_immutable_image_ids_and_metadata_reach_bootstrap_docker_config",
        || {
            Box::pin(verified_immutable_image_ids_and_metadata_reach_bootstrap_docker_config_on_fixture_stack())
        },
    )
}

async fn verified_immutable_image_ids_and_metadata_reach_bootstrap_docker_config_on_fixture_stack()
-> Result<(), String> {
    let failure = crate::launch::steps::runner_create_409().await?;
    let images = failure.images().clone();
    let captured = failure.finish().await?;
    if let Some(error) = captured.server_error {
        return Err(format!("The Docker fixture failed. Error: {error}."));
    }
    let creates = captured
        .requests
        .iter()
        .filter(|request| {
            request.method == "POST" && request.target.starts_with("/containers/create")
        })
        .map(|request| serde_json::from_slice::<Value>(&request.body))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let dind = created_config(&creates, "dind")?;
    let runner = created_config(&creates, "runner")?;
    if dind["Image"] != images.dind().runtime_id()
        || runner["Image"] != images.runner().runtime_id()
    {
        return Err("Bootstrap did not use the verified runtime image IDs.".to_owned());
    }
    if runner["Env"] != serde_json::json!(["DOCKER_HOST=tcp://127.0.0.1:2375"]) {
        return Err("The Runner create request did not pin the private DinD endpoint.".to_owned());
    }
    let runner_mounts = runner["HostConfig"]["Mounts"]
        .as_array()
        .ok_or_else(|| "The Runner storage mounts are absent.".to_owned())?;
    if runner_mounts.iter().any(|mount| {
        mount["Type"] != "volume"
            || mount["Source"].as_str().is_none_or(|source| {
                source.contains("/Users/")
                    || source.contains("/home/")
                    || source.contains("docker.sock")
            })
    }) {
        return Err("Bootstrap supplied a host path instead of a journaled volume.".to_owned());
    }
    let facts = runner_mounts
        .iter()
        .find(|mount| mount["Target"] == "/opt/velnor/job-facts")
        .ok_or_else(|| "The Runner JobFacts volume mount is absent.".to_owned())?;
    if facts["ReadOnly"] != true || facts["VolumeOptions"]["NoCopy"] != true {
        return Err("The Runner JobFacts mount is not read-only and no-copy.".to_owned());
    }
    let dind_mounts = dind["HostConfig"]["Mounts"]
        .as_array()
        .ok_or_else(|| "The DinD storage mounts are absent.".to_owned())?;
    if dind_mounts
        .iter()
        .any(|mount| mount["Target"] == "/opt/velnor/guest-image.json")
    {
        return Err("Docker-in-Docker received the Runner guest metadata file.".to_owned());
    }
    Ok(())
}

fn created_config(creates: &[Value], role: &str) -> Result<Value, String> {
    let matching = creates
        .iter()
        .filter(|config| config["Labels"]["io.velnor.role"].as_str() == Some(role))
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(format!("Expected one {role} create request."));
    }
    Ok(matching[0].clone())
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
