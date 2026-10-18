use serde_json::{Value, json};

use crate::journal::EngineContext;
use crate::launch::SelectedEngineContext;
use crate::launch::inspect_tests::{DockerStub, http};
use crate::launch::verified_images::{
    TestWorkerImageMutation, VerifiedImageIdentity, VerifiedWorkerImages,
    test_worker_images_for_root, test_worker_images_for_root_with_mutation,
};

use super::verify_worker_images_for_engine;

const DOCKER_ROOT: &str = "/var/lib/docker";

#[tokio::test]
async fn fresh_aarch64_engine_accepts_amd64_images_from_both_image_stores() -> Result<(), String> {
    let images = test_worker_images_for_root_with_mutation(
        DOCKER_ROOT,
        Some(TestWorkerImageMutation::RunnerClassicStore),
    );
    let expected = selected_context(&images)?;
    let docker = DockerStub::open(vec![
        http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
        http(
            200,
            &image_inspect(&images, "velnor-runner", images.runner(), true),
        ),
        http(
            200,
            &image_inspect(&images, "velnor-dind", images.dind(), false),
        ),
        http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
    ])?;

    let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
    let requests = docker.finish().await?;

    assert_eq!(result, Ok(()));
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("GET ") && requests[0].contains("/info"));
    assert!(requests[1].contains("/images/"));
    assert!(requests[2].contains("/images/"));
    assert!(requests[3].starts_with("GET ") && requests[3].contains("/info"));
    Ok(())
}

#[tokio::test]
async fn changed_engine_after_image_inspections_rejects_the_bundle() -> Result<(), String> {
    let images = test_worker_images_for_root(DOCKER_ROOT);
    let expected = selected_context(&images)?;
    let docker = DockerStub::open(vec![
        http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
        http(
            200,
            &image_inspect(&images, "velnor-runner", images.runner(), false),
        ),
        http(
            200,
            &image_inspect(&images, "velnor-dind", images.dind(), false),
        ),
        http(
            200,
            &engine_info("replacement-engine", DOCKER_ROOT, "aarch64"),
        ),
    ])?;

    let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
    let requests = docker.finish().await?;

    assert!(result.is_err());
    assert_eq!(requests.len(), 4);
    assert!(requests[0].contains("/info"));
    assert!(requests[1].contains("/images/"));
    assert!(requests[2].contains("/images/"));
    assert!(requests[3].contains("/info"));
    Ok(())
}

#[tokio::test]
async fn changed_engine_root_or_architecture_stops_before_image_inspection() -> Result<(), String> {
    let images = test_worker_images_for_root(DOCKER_ROOT);
    let expected = selected_context(&images)?;
    for (engine, root, architecture) in [
        ("replacement-engine", DOCKER_ROOT, "aarch64"),
        ("selected-engine", "/different/root", "aarch64"),
        ("selected-engine", DOCKER_ROOT, "x86_64"),
    ] {
        let docker = DockerStub::open(vec![http(200, &engine_info(engine, root, architecture))])?;
        let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
        let requests = docker.finish().await?;

        assert!(result.is_err());
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("/info"));
    }
    Ok(())
}

#[tokio::test]
async fn changed_image_id_or_tag_stops_before_the_second_image_inspection() -> Result<(), String> {
    let images = test_worker_images_for_root(DOCKER_ROOT);
    let expected = selected_context(&images)?;
    for mutate in [MutateInspection::ImageId, MutateInspection::RepoTag] {
        let mut inspection: Value = serde_json::from_str(&image_inspect(
            &images,
            "velnor-runner",
            images.runner(),
            false,
        ))
        .map_err(|error| error.to_string())?;
        match mutate {
            MutateInspection::ImageId => {
                inspection["Id"] = json!(format!("sha256:{}", "1".repeat(64)));
            }
            MutateInspection::RepoTag => inspection["RepoTags"] = json!(["velnor-runner:mutable"]),
        }
        let docker = DockerStub::open(vec![
            http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
            http(
                200,
                &serde_json::to_string(&inspection).map_err(|error| error.to_string())?,
            ),
        ])?;

        let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
        let requests = docker.finish().await?;

        assert!(result.is_err());
        assert_eq!(requests.len(), 2);
    }
    Ok(())
}

#[tokio::test]
async fn classic_store_rejects_nonempty_repo_digests() -> Result<(), String> {
    let images = test_worker_images_for_root_with_mutation(
        DOCKER_ROOT,
        Some(TestWorkerImageMutation::RunnerClassicStore),
    );
    let expected = selected_context(&images)?;
    let mut inspection: Value = serde_json::from_str(&image_inspect(
        &images,
        "velnor-runner",
        images.runner(),
        true,
    ))
    .map_err(|error| error.to_string())?;
    inspection["RepoDigests"] = json!([format!(
        "foreign.invalid/velnor-runner@{}",
        images.runner().config_digest()
    )]);
    let docker = DockerStub::open(vec![
        http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
        http(
            200,
            &serde_json::to_string(&inspection).map_err(|error| error.to_string())?,
        ),
    ])?;

    let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
    let requests = docker.finish().await?;

    assert!(result.is_err());
    assert_eq!(requests.len(), 2);
    Ok(())
}

#[tokio::test]
async fn accepts_only_exact_bare_or_docker_hub_local_image_names() -> Result<(), String> {
    let images = test_worker_images_for_root(DOCKER_ROOT);
    let expected = selected_context(&images)?;
    let identity = images.runner();
    let reference = format!(
        "velnor-runner:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    let digest = identity
        .image_index_digest()
        .expect("worker has selected index");
    let accepted = [
        (
            reference.clone(),
            format!("docker.io/library/velnor-runner@{digest}"),
        ),
        (
            format!("docker.io/library/{reference}"),
            format!("docker.io/library/velnor-runner@{digest}"),
        ),
        (reference.clone(), format!("velnor-runner@{digest}")),
        (
            format!("docker.io/library/{reference}"),
            format!("velnor-runner@{digest}"),
        ),
    ];
    for (tag, repo_digest) in accepted {
        let mut inspection: Value =
            serde_json::from_str(&image_inspect(&images, "velnor-runner", identity, false))
                .map_err(|error| error.to_string())?;
        inspection["RepoTags"] = json!([tag]);
        inspection["RepoDigests"] = json!([repo_digest]);
        let docker = DockerStub::open(vec![
            http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
            http(
                200,
                &serde_json::to_string(&inspection).map_err(|error| error.to_string())?,
            ),
            http(
                200,
                &image_inspect(&images, "velnor-dind", images.dind(), false),
            ),
            http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
        ])?;

        let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
        let requests = docker.finish().await?;

        assert_eq!(result, Ok(()));
        assert_eq!(requests.len(), 4);
        assert!(requests[1].contains("/images/"));
    }

    let invalid_tags = [
        format!("ghcr.io/library/{reference}"),
        format!("docker.io/other/{reference}"),
        format!("{reference}-foreign"),
        "velnor-runner:mutable".to_owned(),
    ];
    let invalid_digests = [
        format!("ghcr.io/library/velnor-runner@{digest}"),
        format!("docker.io/other/velnor-runner@{digest}"),
        format!("docker.io/library/velnor-runner-foreign@{digest}"),
        format!("velnor-runner@sha256:{}", "0".repeat(64)),
    ];
    for (tag, repo_digest) in invalid_tags
        .into_iter()
        .map(|tag| (tag, format!("docker.io/library/velnor-runner@{digest}")))
        .chain(
            invalid_digests
                .into_iter()
                .map(|repo_digest| (reference.clone(), repo_digest)),
        )
    {
        let mut inspection: Value =
            serde_json::from_str(&image_inspect(&images, "velnor-runner", identity, false))
                .map_err(|error| error.to_string())?;
        inspection["RepoTags"] = json!([tag]);
        inspection["RepoDigests"] = json!([repo_digest]);
        let docker = DockerStub::open(vec![
            http(200, &engine_info("selected-engine", DOCKER_ROOT, "aarch64")),
            http(
                200,
                &serde_json::to_string(&inspection).map_err(|error| error.to_string())?,
            ),
        ])?;

        let result = verify_worker_images_for_engine(&docker.docker, &expected, &images).await;
        let requests = docker.finish().await?;

        assert!(result.is_err());
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("/images/"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum MutateInspection {
    ImageId,
    RepoTag,
}

fn selected_context(images: &VerifiedWorkerImages) -> Result<SelectedEngineContext, String> {
    SelectedEngineContext::bind(
        &EngineContext {
            engine_id: images.engine_id().to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: images.platform().to_owned(),
        },
        images,
    )
    .map_err(|error| error.to_string())
}

fn engine_info(engine_id: &str, docker_root: &str, architecture: &str) -> String {
    json!({
        "ID": engine_id,
        "OSType": "linux",
        "Architecture": architecture,
        "DockerRootDir": docker_root
    })
    .to_string()
}

fn image_inspect(
    images: &VerifiedWorkerImages,
    image_name: &str,
    identity: &VerifiedImageIdentity,
    classic: bool,
) -> String {
    let reference = format!(
        "{image_name}:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    let mut inspection = json!({
        "Id": identity.runtime_id(),
        "Os": "linux",
        "Architecture": "amd64",
        "RepoTags": [reference]
    });
    if classic {
        inspection["Id"] = json!(identity.config_digest());
        inspection["RepoDigests"] = json!([]);
    } else {
        let index = identity
            .image_index_digest()
            .expect("worker has selected index");
        let tag = reference.split_once(':').expect("reference has tag").1;
        inspection["RepoDigests"] = json!([format!("docker.io/library/{image_name}@{index}")]);
        inspection["Descriptor"] = json!({
            "digest": index,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "size": identity.image_index_size().expect("worker index has size"),
            "annotations": {
                "io.containerd.image.name": format!("docker.io/library/{reference}"),
                "org.opencontainers.image.ref.name": tag
            }
        });
    }
    inspection.to_string()
}
