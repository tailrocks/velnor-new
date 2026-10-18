use std::collections::HashMap;

use bollard::models::{ImageConfig, ImageInspect, OciDescriptor, SystemInfo};

use super::super::identity::image_observation;
use super::super::{
    ProviderFailure, candidate, daemon_architecture, daemon_platform_matches, selected_daemon,
    verify_daemon_image,
};
use super::prepared_release;

#[test]
fn containerd_store_accepts_the_verified_manifest_identity_and_descriptor() {
    let revision = "a".repeat(40);
    let release = prepared_release(&revision);
    let (_, reference_tag) = release.image_reference.split_once(':').unwrap_or_default();
    let image = ImageInspect {
        id: Some(release.manifest_digest.clone()),
        repo_tags: Some(vec![release.image_reference.clone()]),
        descriptor: Some(OciDescriptor {
            media_type: Some("application/vnd.oci.image.manifest.v1+json".to_owned()),
            digest: Some(release.manifest_digest.clone()),
            size: Some(480),
            annotations: Some(HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    format!("docker.io/library/{}", release.image_reference),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    reference_tag.to_owned(),
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
    assert!(image_observation(&image).is_ok());
    assert!(candidate("selected-engine", "x86_64", image, &release).is_ok());
}

#[test]
fn classic_image_observation_rejects_any_repo_digest() {
    let release = prepared_release(&"a".repeat(40));
    let image = ImageInspect {
        id: Some(release.config_digest.clone()),
        repo_tags: Some(vec![release.image_reference.clone()]),
        repo_digests: Some(vec![format!(
            "foreign.invalid/velnor-resource-probe@{}",
            release.config_digest
        )]),
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

    assert_eq!(
        image_observation(&image),
        Err(ProviderFailure::ImageIdentity)
    );
}

#[test]
fn mismatched_store_identity_descriptor_and_index_digest_fail_closed() {
    let revision = "a".repeat(40);
    let release = prepared_release(&revision);
    let (_, reference_tag) = release.image_reference.split_once(':').unwrap_or_default();
    let containerd = ImageInspect {
        id: Some(release.manifest_digest.clone()),
        repo_tags: Some(vec![release.image_reference.clone()]),
        descriptor: Some(OciDescriptor {
            media_type: Some("application/vnd.oci.image.manifest.v1+json".to_owned()),
            digest: Some(release.manifest_digest.clone()),
            size: Some(480),
            annotations: Some(HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    format!("docker.io/library/{}", release.image_reference),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    reference_tag.to_owned(),
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
                id: Some(release.archive_index_digest.clone()),
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
    assert!(!daemon_platform_matches(&unknown));
}
