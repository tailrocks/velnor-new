use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bollard::models::ContainerConfig;
use serde_json::{Value, json};

use crate::journal::{ImageImportState, Journal};
use crate::launch::inspect_tests::{DockerStub, http};
use crate::launch::resource_probe::projection::DockerRoot;
use crate::launch_harness::Scratch;

use super::super::archive::worker::{DerivedLoadArchive, ParsedWorkerArchive, WorkerImage};
use super::super::identity::SelectedDaemon;
use super::super::release::PreparedWorkerArtifact;
use super::{ProviderFailure, import_worker_artifact};

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DOCKER_ROOT: &str = "/var/lib/docker";

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
async fn exact_preloaded_worker_image_completes_without_loading() -> Result<(), String> {
    let (_scratch, journal) = journal("preloaded").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let selected = selected_daemon();
    let artifact = artifact();
    let docker = DockerStub::open(vec![
        http(200, &image_inspect(&artifact, None)),
        http(200, &engine_info()),
    ])?;

    let proof = import_worker_artifact(&docker.docker, &journal, &selected, SOURCE_SHA, &artifact)
        .await
        .map_err(|error| format!("preloaded image verification failed: {error:?}"))?;
    let requests = docker.finish().await?;
    assert_eq!(proof.runtime_id(), format!("sha256:{}", "3".repeat(64)));
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("/images/"));
    assert!(requests[1].contains("/info"));
    let record = journal
        .prepare_image_import(
            super::import_request(&selected, SOURCE_SHA, &artifact)
                .map_err(|error| format!("request failed: {error:?}"))?,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(record.state, ImageImportState::Completed);
    assert!(record.receipt.is_some());
    Ok(())
}

#[tokio::test]
async fn completed_worker_image_requires_the_same_durable_receipt() -> Result<(), String> {
    let (_scratch, journal) = journal("completed-receipt").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let selected = selected_daemon();
    let artifact = artifact();
    let bare_docker = DockerStub::open(vec![
        http(200, &image_inspect(&artifact, None)),
        http(200, &engine_info()),
    ])?;
    import_worker_artifact(
        &bare_docker.docker,
        &journal,
        &selected,
        SOURCE_SHA,
        &artifact,
    )
    .await
    .map_err(|error| format!("bare local image receipt failed: {error:?}"))?;
    bare_docker.finish().await?;

    let request = super::import_request(&selected, SOURCE_SHA, &artifact)
        .map_err(|error| format!("request failed: {error:?}"))?;
    let completed = journal
        .prepare_image_import(request)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(completed.state, ImageImportState::Completed);
    let original_receipt = completed
        .receipt
        .ok_or_else(|| "completed image receipt missing".to_owned())?;

    let qualified_docker = DockerStub::open(vec![
        http(200, &image_inspect_with_spelling(&artifact, None, true)),
        http(200, &engine_info()),
    ])?;
    let replay = import_worker_artifact(
        &qualified_docker.docker,
        &journal,
        &selected,
        SOURCE_SHA,
        &artifact,
    )
    .await;
    let requests = qualified_docker.finish().await?;
    assert!(matches!(replay, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 2);

    let after = journal
        .prepare_image_import(
            super::import_request(&selected, SOURCE_SHA, &artifact)
                .map_err(|error| format!("request failed: {error:?}"))?,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(after.state, ImageImportState::Completed);
    assert_eq!(after.receipt, Some(original_receipt));
    Ok(())
}

#[tokio::test]
async fn config_mismatch_stops_before_daemon_binding_or_load() -> Result<(), String> {
    let (_scratch, journal) = journal("config-mismatch").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let selected = selected_daemon();
    let artifact = artifact();
    let docker = DockerStub::open(vec![http(
        200,
        &image_inspect(&artifact, Some("different-user")),
    )])?;

    let result =
        import_worker_artifact(&docker.docker, &journal, &selected, SOURCE_SHA, &artifact).await;
    let requests = docker.finish().await?;
    assert!(matches!(result, Err(ProviderFailure::ImageConfig)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("/images/"));
    let record = journal
        .prepare_image_import(
            super::import_request(&selected, SOURCE_SHA, &artifact)
                .map_err(|error| format!("request failed: {error:?}"))?,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(record.state, ImageImportState::Prepared);
    Ok(())
}

#[tokio::test]
async fn wrong_rootfs_layers_stop_before_durable_completion() -> Result<(), String> {
    let (_scratch, journal) = journal("rootfs-mismatch").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let selected = selected_daemon();
    let artifact = artifact();
    let docker = DockerStub::open(vec![http(
        200,
        &image_inspect_with_layers(&artifact, &["sha256:bad".to_owned()], None),
    )])?;

    let result =
        import_worker_artifact(&docker.docker, &journal, &selected, SOURCE_SHA, &artifact).await;
    let requests = docker.finish().await?;
    assert!(matches!(result, Err(ProviderFailure::ImageConfig)));
    assert_eq!(requests.len(), 1);
    let record = journal
        .prepare_image_import(
            super::import_request(&selected, SOURCE_SHA, &artifact)
                .map_err(|error| format!("request failed: {error:?}"))?,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(record.state, ImageImportState::Prepared);
    Ok(())
}

#[tokio::test]
async fn classic_image_with_repo_digest_stays_prepared() -> Result<(), String> {
    let (_scratch, journal) = journal("classic-repo-digest").await?;
    journal
        .bind_engine("selected-engine")
        .await
        .map_err(|error| error.to_string())?;
    let selected = selected_daemon();
    let artifact = artifact();
    let mut inspection: Value =
        serde_json::from_str(&image_inspect(&artifact, None)).map_err(|error| error.to_string())?;
    inspection["Id"] = json!(artifact.original.config_digest);
    inspection["Descriptor"] = Value::Null;
    inspection["RepoDigests"] = json!([format!(
        "foreign.invalid/velnor-runner@{}",
        artifact.original.config_digest
    )]);
    let docker = DockerStub::open(vec![http(
        200,
        &serde_json::to_string(&inspection).map_err(|error| error.to_string())?,
    )])?;

    let result =
        import_worker_artifact(&docker.docker, &journal, &selected, SOURCE_SHA, &artifact).await;
    let requests = docker.finish().await?;
    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 1);
    let record = journal
        .prepare_image_import(
            super::import_request(&selected, SOURCE_SHA, &artifact)
                .map_err(|error| format!("request failed: {error:?}"))?,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(record.state, ImageImportState::Prepared);
    assert!(record.receipt.is_none());
    Ok(())
}

fn artifact() -> PreparedWorkerArtifact {
    let archive = ParsedWorkerArchive::test_fixture(
        config(None),
        vec![format!("sha256:{}", "6".repeat(64))],
        WorkerImage::Runner.release_tag(),
    );
    PreparedWorkerArtifact {
        image: WorkerImage::Runner,
        original: archive.clone(),
        load: DerivedLoadArchive {
            path: next_path(),
            sha256: "7".repeat(64),
            archive_index_digest: "8".repeat(64),
            repo_tag: format!("velnor-runner:v{SOURCE_SHA}-{}", archive.archive_sha256),
            parsed: archive,
        },
    }
}

fn config(user: Option<&str>) -> ContainerConfig {
    ContainerConfig {
        user: user
            .map(str::to_owned)
            .or_else(|| Some("runner".to_owned())),
        env: Some(vec!["PATH=/usr/bin".to_owned()]),
        entrypoint: Some(vec!["/usr/local/bin/velnor-runner-entrypoint".to_owned()]),
        working_dir: Some("/home/runner".to_owned()),
        ..ContainerConfig::default()
    }
}

fn image_inspect(artifact: &PreparedWorkerArtifact, user: Option<&str>) -> String {
    image_inspect_with_spelling_and_layers(artifact, &artifact.original.diff_ids, user, false)
}

fn image_inspect_with_spelling(
    artifact: &PreparedWorkerArtifact,
    user: Option<&str>,
    qualified: bool,
) -> String {
    image_inspect_with_spelling_and_layers(artifact, &artifact.original.diff_ids, user, qualified)
}

fn image_inspect_with_layers(
    artifact: &PreparedWorkerArtifact,
    rootfs_layers: &[String],
    user: Option<&str>,
) -> String {
    image_inspect_with_spelling_and_layers(artifact, rootfs_layers, user, false)
}

fn image_inspect_with_spelling_and_layers(
    artifact: &PreparedWorkerArtifact,
    rootfs_layers: &[String],
    user: Option<&str>,
    qualified: bool,
) -> String {
    let reference = &artifact.load.repo_tag;
    let tag = reference
        .split_once(':')
        .map(|(_, tag)| tag)
        .expect("derived reference has a tag");
    let index = &artifact.original.image_index_digest;
    let image_config = config(user);
    let observed_reference = if qualified {
        format!("docker.io/library/{reference}")
    } else {
        reference.clone()
    };
    let observed_repo_digest = if qualified {
        format!("docker.io/library/velnor-runner@{index}")
    } else {
        format!("velnor-runner@{index}")
    };
    json!({
        "Id": index,
        "Descriptor": {
            "digest": index,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "size": artifact.original.image_index_size,
            "annotations": {
                "io.containerd.image.name": format!("docker.io/library/{reference}"),
                "org.opencontainers.image.ref.name": tag
            }
        },
        "RepoTags": [observed_reference],
        "RepoDigests": [observed_repo_digest],
        "Os": "linux",
        "Architecture": "amd64",
        "Config": serde_json::to_value(image_config).expect("image config serializes"),
        "RootFS": { "Type": "layers", "Layers": rootfs_layers }
    })
    .to_string()
}

fn engine_info() -> String {
    json!({
        "ID": "selected-engine",
        "OSType": "linux",
        "Architecture": "aarch64",
        "DockerRootDir": DOCKER_ROOT
    })
    .to_string()
}

fn selected_daemon() -> SelectedDaemon {
    SelectedDaemon {
        engine_id: "selected-engine".to_owned(),
        docker_root_digest: DockerRoot::parse(DOCKER_ROOT)
            .expect("Docker root parses")
            .digest()
            .to_owned(),
        architecture: "aarch64".to_owned(),
    }
}

async fn journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}

fn next_path() -> PathBuf {
    let sequence = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "velnor-worker-import-{}-{sequence}.tar",
        std::process::id()
    ))
}
