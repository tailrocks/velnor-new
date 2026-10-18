//! Regressions for derived worker load identity and receipt validation.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::HostError;
use crate::journal::{
    ImageImportDescriptor, ImageImportIdentity, ImageImportInspection, ImageImportObservation,
    ImageImportRequest, Journal,
};

const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-image-identity-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn worker_request() -> Result<ImageImportRequest, HostError> {
    let source = "a".repeat(40);
    let archive = "c".repeat(64);
    ImageImportRequest::new(ImageImportIdentity {
        engine_id: "engine-a".to_owned(),
        docker_root_digest: "b".repeat(64),
        daemon_architecture: "x86_64".to_owned(),
        source_sha: source.clone(),
        archive_sha256: archive.clone(),
        archive_index_digest: "d".repeat(64),
        platform: "linux/amd64".to_owned(),
        manifest_digest: format!("sha256:{}", "e".repeat(64)),
        manifest_size: 4472,
        config_digest: format!("sha256:{}", "f".repeat(64)),
        image_index_digest: Some(format!("sha256:{}", "1".repeat(64))),
        image_index_size: Some(856),
        load_archive_sha256: Some("2".repeat(64)),
        load_archive_index_digest: Some("3".repeat(64)),
        image_reference: format!("velnor-runner:v{source}-{archive}"),
    })
}

fn derived_probe_identity() -> ImageImportIdentity {
    let source = "a".repeat(40);
    let archive = "c".repeat(64);
    ImageImportIdentity {
        engine_id: "engine-a".to_owned(),
        docker_root_digest: "b".repeat(64),
        daemon_architecture: "aarch64".to_owned(),
        source_sha: source.clone(),
        archive_sha256: archive.clone(),
        archive_index_digest: "d".repeat(64),
        platform: "linux/amd64".to_owned(),
        manifest_digest: format!("sha256:{}", "e".repeat(64)),
        manifest_size: 480,
        config_digest: format!("sha256:{}", "f".repeat(64)),
        image_index_digest: None,
        image_index_size: None,
        load_archive_sha256: Some("1".repeat(64)),
        load_archive_index_digest: Some("2".repeat(64)),
        image_reference: format!("velnor-resource-probe:v{source}-{archive}"),
    }
}

fn descriptor(request: &ImageImportRequest) -> ImageImportDescriptor {
    let (name, tag) = request.image_reference.split_once(':').unwrap_or_default();
    ImageImportDescriptor {
        digest: request.image_index_digest.clone().unwrap_or_default(),
        media_type: OCI_INDEX.to_owned(),
        size: 856,
        annotations: BTreeMap::from([
            (
                "io.containerd.image.name".to_owned(),
                format!("docker.io/library/{name}:{tag}"),
            ),
            (
                "org.opencontainers.image.ref.name".to_owned(),
                tag.to_owned(),
            ),
        ]),
        has_extra_fields: false,
    }
}

fn containerd_observation(request: &ImageImportRequest) -> ImageImportObservation {
    let descriptor = descriptor(request);
    ImageImportObservation::Containerd {
        image_id: descriptor.digest.clone(),
        repo_tags: vec![request.image_reference.clone()],
        repo_digests: vec![format!(
            "docker.io/library/velnor-runner@{}",
            descriptor.digest
        )],
        descriptor,
    }
}

#[test]
fn nested_index_receipt_binds_content_addressed_tag_and_descriptor() -> Result<(), HostError> {
    let request = worker_request()?;
    let observation = containerd_observation(&request);
    let receipt = ImageImportInspection::verified(
        &request,
        request.engine_id(),
        request.docker_root_digest(),
        request.daemon_architecture(),
        observation.clone(),
    )?;
    observation.validate_for(&request)?;
    assert_eq!(
        receipt.image_id(),
        request.image_index_digest.as_deref().unwrap_or_default()
    );
    Ok(())
}

#[test]
fn nested_index_rejects_foreign_tags_and_wrong_descriptor_identity() -> Result<(), HostError> {
    let request = worker_request()?;
    let mut foreign = containerd_observation(&request);
    if let ImageImportObservation::Containerd { repo_tags, .. } = &mut foreign {
        repo_tags.push("velnor-runner:latest".to_owned());
    }
    assert!(foreign.validate_for(&request).is_err());

    let mut wrong_index = containerd_observation(&request);
    if let ImageImportObservation::Containerd { descriptor, .. } = &mut wrong_index {
        descriptor.digest = format!("sha256:{}", "9".repeat(64));
    }
    assert!(wrong_index.validate_for(&request).is_err());
    Ok(())
}

#[test]
fn accepts_only_the_two_local_tag_and_digest_spellings_and_keeps_evidence() -> Result<(), HostError>
{
    let request = worker_request()?;
    let (name, _) = request.image_reference.split_once(':').unwrap_or_default();
    let digest = request.image_index_digest.as_deref().unwrap_or_default();
    for tag in [
        request.image_reference.clone(),
        format!("docker.io/library/{}", request.image_reference),
    ] {
        for repo_digest in [
            format!("{name}@{digest}"),
            format!("docker.io/library/{name}@{digest}"),
        ] {
            let mut observation = containerd_observation(&request);
            if let ImageImportObservation::Containerd {
                repo_tags,
                repo_digests,
                ..
            } = &mut observation
            {
                *repo_tags = vec![tag.clone()];
                *repo_digests = vec![repo_digest.clone()];
            }
            let receipt = ImageImportInspection::verified(
                &request,
                request.engine_id(),
                request.docker_root_digest(),
                request.daemon_architecture(),
                observation.clone(),
            )?;
            assert_eq!(receipt.image_id(), digest);
            let serialized = observation.receipt()?;
            assert_eq!(
                ImageImportObservation::from_receipt(&serialized)?,
                observation
            );
            assert!(serialized.contains(&tag));
            assert!(serialized.contains(&repo_digest));
        }
    }
    Ok(())
}

#[test]
fn rejects_nonlocal_or_extended_tag_and_repo_digest_spellings() -> Result<(), HostError> {
    let request = worker_request()?;
    let (name, _) = request.image_reference.split_once(':').unwrap_or_default();
    let digest = request.image_index_digest.as_deref().unwrap_or_default();
    for tag in [
        format!("ghcr.io/library/{}", request.image_reference),
        format!("docker.io/other/{}", request.image_reference),
        format!("{}-foreign", request.image_reference),
    ] {
        let mut observation = containerd_observation(&request);
        if let ImageImportObservation::Containerd { repo_tags, .. } = &mut observation {
            *repo_tags = vec![tag];
        }
        assert!(observation.validate_for(&request).is_err());
    }
    for repo_digest in [
        format!("ghcr.io/library/{name}@{digest}"),
        format!("docker.io/other/{name}@{digest}"),
        format!("{name}@{}0", digest),
    ] {
        let mut observation = containerd_observation(&request);
        if let ImageImportObservation::Containerd { repo_digests, .. } = &mut observation {
            *repo_digests = vec![repo_digest];
        }
        assert!(observation.validate_for(&request).is_err());
    }
    let mut duplicated = containerd_observation(&request);
    if let ImageImportObservation::Containerd { repo_tags, .. } = &mut duplicated {
        repo_tags.push(request.image_reference.clone());
    }
    assert!(duplicated.validate_for(&request).is_err());
    Ok(())
}

#[test]
fn classic_store_requires_config_id_and_exact_worker_tag() -> Result<(), HostError> {
    let request = worker_request()?;
    let observation = ImageImportObservation::Classic {
        image_id: request.config_digest.clone(),
        repo_tags: vec![request.image_reference.clone()],
    };
    observation.validate_for(&request)?;

    let wrong_tag = ImageImportObservation::Classic {
        image_id: request.config_digest.clone(),
        repo_tags: vec!["velnor-runner:latest".to_owned()],
    };
    assert!(wrong_tag.validate_for(&request).is_err());
    Ok(())
}

#[test]
fn direct_probe_uses_manifest_descriptor_without_worker_transport() -> Result<(), HostError> {
    let request = ImageImportRequest::new(ImageImportIdentity {
        engine_id: "engine-a".to_owned(),
        docker_root_digest: "b".repeat(64),
        daemon_architecture: "x86_64".to_owned(),
        source_sha: "a".repeat(40),
        archive_sha256: "c".repeat(64),
        archive_index_digest: "d".repeat(64),
        platform: "linux/amd64".to_owned(),
        manifest_digest: format!("sha256:{}", "e".repeat(64)),
        manifest_size: 128,
        config_digest: format!("sha256:{}", "f".repeat(64)),
        image_index_digest: None,
        image_index_size: None,
        load_archive_sha256: None,
        load_archive_index_digest: None,
        image_reference: "velnor-resource-probe:linux-amd64".to_owned(),
    })?;
    let observation = ImageImportObservation::Containerd {
        image_id: request.manifest_digest.clone(),
        repo_tags: vec![request.image_reference.clone()],
        repo_digests: Vec::new(),
        descriptor: ImageImportDescriptor {
            digest: request.manifest_digest.clone(),
            media_type: OCI_MANIFEST.to_owned(),
            size: request.manifest_size,
            annotations: BTreeMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    "docker.io/library/velnor-resource-probe:linux-amd64".to_owned(),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    "linux-amd64".to_owned(),
                ),
            ]),
            has_extra_fields: false,
        },
    };
    observation.validate_for(&request)?;
    Ok(())
}

#[test]
fn direct_probe_accepts_content_addressed_load_with_manifest_identity() -> Result<(), HostError> {
    let request = ImageImportRequest::new(derived_probe_identity())?;
    assert!(request.image_index_digest.is_none());
    assert!(request.load_archive_sha256.is_some());
    assert!(request.load_archive_index_digest.is_some());
    let (name, tag) = request.image_reference.split_once(':').unwrap_or_default();
    let observation = ImageImportObservation::Containerd {
        image_id: request.manifest_digest.clone(),
        repo_tags: vec![request.image_reference.clone()],
        repo_digests: vec![format!(
            "docker.io/library/{name}@{}",
            request.manifest_digest
        )],
        descriptor: ImageImportDescriptor {
            digest: request.manifest_digest.clone(),
            media_type: OCI_MANIFEST.to_owned(),
            size: request.manifest_size,
            annotations: BTreeMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    format!("docker.io/library/{name}:{tag}"),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    tag.to_owned(),
                ),
            ]),
            has_extra_fields: false,
        },
    };
    let receipt = ImageImportInspection::verified(
        &request,
        request.engine_id(),
        request.docker_root_digest(),
        request.daemon_architecture(),
        observation.clone(),
    )?;
    observation.validate_for(&request)?;
    assert_eq!(receipt.image_id(), request.manifest_digest);
    Ok(())
}

#[test]
fn direct_probe_rejects_wrong_prefix_tag_source_and_load_effect_identity() {
    let identity = derived_probe_identity();

    let mut wrong_archive_prefix = identity.clone();
    wrong_archive_prefix.archive_index_digest = format!("sha256:{}", "d".repeat(64));
    assert!(ImageImportRequest::new(wrong_archive_prefix).is_err());

    let mut wrong_load_prefix = identity.clone();
    wrong_load_prefix.load_archive_index_digest = Some(format!("sha256:{}", "2".repeat(64)));
    assert!(ImageImportRequest::new(wrong_load_prefix).is_err());

    let mut wrong_tag = identity.clone();
    wrong_tag.image_reference = format!(
        "velnor-resource-probe:v{}-{}",
        wrong_tag.source_sha,
        "9".repeat(64)
    );
    assert!(ImageImportRequest::new(wrong_tag).is_err());

    let mut wrong_source = identity.clone();
    wrong_source.source_sha = "9".repeat(40);
    assert!(ImageImportRequest::new(wrong_source).is_err());

    let mut missing_effect_hash = identity.clone();
    missing_effect_hash.load_archive_index_digest = None;
    assert!(ImageImportRequest::new(missing_effect_hash).is_err());

    let mut missing_effect_index = identity;
    missing_effect_index.load_archive_sha256 = None;
    assert!(ImageImportRequest::new(missing_effect_index).is_err());
}

#[tokio::test]
async fn changed_derived_transport_cannot_replay_an_original_artifact() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.bind_engine("engine-a").await?;
    let request = worker_request()?;
    journal.prepare_image_import(request.clone()).await?;

    let mut changed = request.clone();
    changed.load_archive_sha256 = Some("4".repeat(64));
    assert_eq!(
        journal.prepare_image_import(changed).await,
        Err(HostError::Identity)
    );
    Ok(())
}
