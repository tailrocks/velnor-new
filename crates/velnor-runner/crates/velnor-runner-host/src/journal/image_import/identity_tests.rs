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

#[tokio::test]
async fn v6_rows_migrate_with_nullable_extension_and_keep_history() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.bind_engine("engine-a").await?;
    drop(journal);

    let text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE image_imports", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE image_imports (id INTEGER PRIMARY KEY AUTOINCREMENT, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), daemon_architecture TEXT NOT NULL CHECK (daemon_architecture IN ('x86_64', 'aarch64')), source_sha TEXT NOT NULL CHECK (length(source_sha) = 40 AND source_sha NOT GLOB '*[^0-9a-f]*'), archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*'), index_digest TEXT NOT NULL CHECK (length(index_digest) = 64 AND index_digest NOT GLOB '*[^0-9a-f]*'), platform TEXT NOT NULL CHECK (platform = 'linux/amd64'), manifest_digest TEXT NOT NULL CHECK (length(manifest_digest) = 71 AND substr(manifest_digest, 1, 7) = 'sha256:' AND substr(manifest_digest, 8) NOT GLOB '*[^0-9a-f]*'), config_digest TEXT NOT NULL CHECK (length(config_digest) = 71 AND substr(config_digest, 1, 7) = 'sha256:' AND substr(config_digest, 8) NOT GLOB '*[^0-9a-f]*'), attempt INTEGER NOT NULL CHECK (attempt >= 0), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'completed', 'aborted')), image_id TEXT, CHECK ((state = 'completed' AND image_id IS NOT NULL) OR (state != 'completed' AND image_id IS NULL)), UNIQUE (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE INDEX image_imports_engine_state ON image_imports(engine_id, state, id)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id) VALUES ('engine-a', ?1, 'x86_64', ?2, ?3, ?4, 'linux/amd64', ?5, ?6, 0, 'completed', ?6)",
            (
                "b".repeat(64),
                "a".repeat(40),
                "c".repeat(64),
                "d".repeat(64),
                format!("sha256:{}", "e".repeat(64)),
                format!("sha256:{}", "f".repeat(64)),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 6", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let journal = Journal::open(&path).await?;
    let mut rows = journal
        .connection()
        .await?
        .query(
            "SELECT state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt FROM image_imports",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "completed"
    );
    assert_eq!(
        row.get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?
            .as_deref(),
        Some(format!("sha256:{}", "f".repeat(64)).as_str())
    );
    for column in [2, 5, 6, 7, 8] {
        assert!(
            row.get::<Option<String>>(column)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    for column in [3, 4] {
        assert!(
            row.get::<Option<i64>>(column)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    assert_eq!(
        journal.prepare_image_import(worker_request()?).await,
        Err(HostError::Identity)
    );
    Ok(())
}
