use std::collections::HashMap;
use std::sync::Arc;

use bollard::Docker;
use bollard::models::{ContainerConfig, ImageConfig, ImageInspect, OciDescriptor, SystemInfo};

use crate::journal::{ImageImportState, Journal};
use crate::launch_harness::Scratch;

use super::release::PreparedRelease;
use super::{
    ProviderFailure, ReleaseImageProvider, build_pin_status, candidate, daemon_architecture,
    selected_daemon, verify_daemon_image,
};
use crate::launch::inspect_tests::{DockerStub, http};

fn prepared_release(source_revision: &str) -> PreparedRelease {
    PreparedRelease {
        archive: Vec::new(),
        source_revision: source_revision.to_owned(),
        index_digest: "b".repeat(64),
        manifest_digest: format!("sha256:{}", "c".repeat(64)),
        manifest_size: 480,
        config_digest: format!("sha256:{}", "a".repeat(64)),
        config: ContainerConfig {
            user: Some("65532:65532".to_owned()),
            env: Some(vec![
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned(),
            ]),
            working_dir: Some("/".to_owned()),
            entrypoint: Some(vec!["/velnor/resource-probe".to_owned()]),
            labels: Some(HashMap::from([(
                "org.opencontainers.image.revision".to_owned(),
                source_revision.to_owned(),
            )])),
            ..Default::default()
        },
    }
}

#[test]
fn provider_requires_a_valid_compiled_source_pin() {
    assert!(matches!(
        ReleaseImageProvider::with_source_revision("pat", "not-a-sha"),
        Err(ProviderFailure::TrustedSourcePinInvalid)
    ));
    assert!(matches!(
        ReleaseImageProvider::with_source_revision("", &"a".repeat(40)),
        Err(ProviderFailure::CredentialMissing)
    ));
    if let Err(reason) = build_pin_status() {
        assert!(matches!(
            reason,
            "trusted_source_pin_missing" | "trusted_source_pin_invalid"
        ));
    }
}

#[tokio::test]
async fn verifier_failure_stops_the_production_provider_path_before_docker() -> Result<(), String> {
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let (_scratch, journal) = test_journal("provider-verifier-failure").await?;
    let docker = Docker::connect_with_defaults().map_err(|error| error.to_string())?;
    let info = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("x86_64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let result = provider
        .verified_candidate_with_release(
            &docker,
            &journal,
            "selected-engine",
            &info,
            std::future::ready(Err(ProviderFailure::Attestation)),
        )
        .await;
    assert!(matches!(result, Err(ProviderFailure::Attestation)));
    Ok(())
}

#[tokio::test]
async fn release_delay_and_engine_change_block_image_inspection_and_import() -> Result<(), String> {
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let (_scratch, journal) = test_journal("provider-engine-change").await?;
    let selected = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("x86_64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let replacement = r#"{"ID":"replacement-engine","OSType":"linux","Architecture":"x86_64","DockerRootDir":"/var/lib/docker"}"#;
    let docker = DockerStub::open(vec![http(200, replacement)])?;
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Ok(Arc::new(prepared_release(&"a".repeat(40))))
    };

    let result = provider
        .verified_candidate_with_release(
            &docker.docker,
            &journal,
            "selected-engine",
            &selected,
            release,
        )
        .await;
    let requests = docker.finish().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET ") && requests[0].contains("/info"));
    Ok(())
}

#[tokio::test]
async fn architecture_change_during_release_preparation_blocks_image_load() -> Result<(), String> {
    let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
        .map_err(|error| error.code().to_owned())?;
    let (_scratch, journal) = test_journal("provider-architecture-change").await?;
    let selected = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("aarch64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    let replacement = r#"{"ID":"selected-engine","OSType":"linux","Architecture":"x86_64","DockerRootDir":"/var/lib/docker"}"#;
    let docker = DockerStub::open(vec![http(200, replacement)])?;
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Ok(Arc::new(prepared_release(&"a".repeat(40))))
    };

    let result = provider
        .verified_candidate_with_release(
            &docker.docker,
            &journal,
            "selected-engine",
            &selected,
            release,
        )
        .await;
    let requests = docker.finish().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET ") && requests[0].contains("/info"));
    Ok(())
}

#[tokio::test]
async fn unresolved_import_rows_do_not_inspect_or_repeat_the_load() -> Result<(), String> {
    for (label, uncertain) in [("requested", false), ("uncertain", true)] {
        let (_scratch, journal) = test_journal(&format!("provider-import-{label}")).await?;
        journal
            .bind_engine("selected-engine")
            .await
            .map_err(|error| error.to_string())?;
        let provider = ReleaseImageProvider::with_source_revision("pat", &"a".repeat(40))
            .map_err(|error| error.code().to_owned())?;
        let release = prepared_release(&"a".repeat(40));
        let info = SystemInfo {
            id: Some("selected-engine".to_owned()),
            os_type: Some("linux".to_owned()),
            architecture: Some("aarch64".to_owned()),
            docker_root_dir: Some("/var/lib/docker".to_owned()),
            ..Default::default()
        };
        let selected =
            selected_daemon(&info, "selected-engine").map_err(|error| error.code().to_owned())?;
        let request = super::image_import_request(&selected, &release)
            .map_err(|error| error.code().to_owned())?;
        let record = journal
            .prepare_image_import(request.clone())
            .await
            .map_err(|error| error.to_string())?;
        let claim = journal
            .claim_image_import_effect(record.id, &request)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "expected prepared image import claim".to_owned())?;
        if uncertain {
            journal
                .mark_image_import_uncertain(claim)
                .await
                .map_err(|error| error.to_string())?;
        }

        let docker = DockerStub::open(Vec::new())?;
        let result = provider
            .inspect_or_load(&docker.docker, &journal, &selected, &release)
            .await;
        let requests = docker.finish_observed().await?;
        assert!(matches!(result, Err(ProviderFailure::ImageLoad)));
        assert_eq!(requests, Vec::<String>::new());
        let replay = journal
            .prepare_image_import(request)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(
            replay.state,
            if uncertain {
                ImageImportState::Uncertain
            } else {
                ImageImportState::Requested
            }
        );
    }
    Ok(())
}

#[test]
fn containerd_store_accepts_the_verified_manifest_identity_and_descriptor() {
    let revision = "a".repeat(40);
    let release = prepared_release(&revision);
    let image = ImageInspect {
        id: Some(release.manifest_digest.clone()),
        descriptor: Some(OciDescriptor {
            media_type: Some("application/vnd.oci.image.manifest.v1+json".to_owned()),
            digest: Some(release.manifest_digest.clone()),
            size: Some(480),
            annotations: Some(HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    "docker.io/library/velnor-resource-probe:linux-amd64".to_owned(),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    "linux-amd64".to_owned(),
                ),
            ])),
            ..Default::default()
        }),
        os: Some("linux".to_owned()),
        architecture: Some("amd64".to_owned()),
        config: Some(ImageConfig {
            user: release.config.user.clone(),
            env: release.config.env.clone(),
            working_dir: release.config.working_dir.clone(),
            entrypoint: release.config.entrypoint.clone(),
            labels: release.config.labels.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(verify_daemon_image(&image, &release).is_ok());
    let daemon = SystemInfo {
        id: Some("selected-engine".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some("aarch64".to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..Default::default()
    };
    assert_eq!(daemon_architecture(&daemon), Some("aarch64"));
    assert!(selected_daemon(&daemon, "selected-engine").is_ok());
    let candidate = candidate("selected-engine", "aarch64", image, &release);
    assert!(candidate.is_ok());
    if let Ok(candidate) = candidate {
        assert_eq!(candidate.platform, "linux/amd64");
        assert_eq!(candidate.daemon_architecture, "aarch64");
    }
}

#[test]
fn exact_daemon_identity_and_config_are_required() {
    let revision = "a".repeat(40);
    let release = prepared_release(&revision);
    let image = ImageInspect {
        id: Some(release.config_digest.clone()),
        os: Some("linux".to_owned()),
        architecture: Some("amd64".to_owned()),
        config: Some(ImageConfig {
            user: release.config.user.clone(),
            env: release.config.env.clone(),
            working_dir: release.config.working_dir.clone(),
            entrypoint: release.config.entrypoint.clone(),
            labels: release.config.labels.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(verify_daemon_image(&image, &release).is_ok());

    let bad_config = ImageInspect {
        config: Some(ImageConfig::default()),
        ..image
    };
    assert_eq!(
        verify_daemon_image(&bad_config, &release),
        Err(ProviderFailure::ImageConfig)
    );
    assert!(candidate("selected-engine", "x86_64", bad_config, &release).is_err());
}

#[test]
fn classic_store_accepts_only_the_verified_config_identity_without_descriptor() {
    let revision = "a".repeat(40);
    let release = prepared_release(&revision);
    let image = ImageInspect {
        id: Some(release.config_digest.clone()),
        os: Some("linux".to_owned()),
        architecture: Some("amd64".to_owned()),
        config: Some(ImageConfig {
            user: release.config.user.clone(),
            env: release.config.env.clone(),
            working_dir: release.config.working_dir.clone(),
            entrypoint: release.config.entrypoint.clone(),
            labels: release.config.labels.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(verify_daemon_image(&image, &release).is_ok());
    assert!(candidate("selected-engine", "x86_64", image, &release).is_ok());
}

#[test]
fn mismatched_store_identity_descriptor_and_index_digest_fail_closed() {
    let revision = "a".repeat(40);
    let release = prepared_release(&revision);
    let containerd = ImageInspect {
        id: Some(release.manifest_digest.clone()),
        descriptor: Some(OciDescriptor {
            media_type: Some("application/vnd.oci.image.manifest.v1+json".to_owned()),
            digest: Some(release.manifest_digest.clone()),
            size: Some(480),
            annotations: Some(HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    "docker.io/library/velnor-resource-probe:linux-amd64".to_owned(),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    "linux-amd64".to_owned(),
                ),
            ])),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        verify_daemon_image(
            &ImageInspect {
                descriptor: Some(OciDescriptor {
                    digest: Some(release.config_digest.clone()),
                    ..containerd.descriptor.clone().unwrap_or_default()
                }),
                ..containerd.clone()
            },
            &release
        ),
        Err(ProviderFailure::ImageIdentity)
    );
    assert_eq!(
        verify_daemon_image(
            &ImageInspect {
                descriptor: Some(OciDescriptor {
                    size: Some(481),
                    ..containerd.descriptor.clone().unwrap_or_default()
                }),
                ..containerd.clone()
            },
            &release
        ),
        Err(ProviderFailure::ImageIdentity)
    );
    assert_eq!(
        verify_daemon_image(
            &ImageInspect {
                descriptor: None,
                id: Some(release.index_digest.clone()),
                ..containerd.clone()
            },
            &release
        ),
        Err(ProviderFailure::ImageIdentity)
    );
    assert_eq!(
        verify_daemon_image(
            &ImageInspect {
                descriptor: None,
                id: Some(release.manifest_digest.clone()),
                ..containerd
            },
            &release
        ),
        Err(ProviderFailure::ImageIdentity)
    );
}

#[test]
fn provider_does_not_accept_unknown_daemon_platform() {
    let unknown = SystemInfo::default();
    assert!(!super::daemon_platform_matches(&unknown));
}

async fn test_journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}
