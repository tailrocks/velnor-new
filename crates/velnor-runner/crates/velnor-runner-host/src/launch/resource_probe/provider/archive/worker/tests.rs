use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tar::{Builder, EntryType};

use super::{WorkerImage, derive_load_archive, new_header, sha256, verify};

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    archive: Vec<u8>,
    layer_blobs: Vec<(String, Vec<u8>)>,
    expected_manifest: String,
}

fn fixture(image: WorkerImage) -> Fixture {
    let layer = b"fixture compressed layer".to_vec();
    let diff_id = format!("sha256:{}", "1".repeat(64));
    let config = runtime_config(image);
    let image_config = json!({
        "architecture": "amd64",
        "os": "linux",
        "config": config,
        "rootfs": { "type": "layers", "diff_ids": [diff_id] }
    });
    let image_config_bytes = serde_json::to_vec(&image_config).expect("image config serializes");
    let config_descriptor = descriptor(
        "application/vnd.oci.image.config.v1+json",
        &image_config_bytes,
    );
    let layer_descriptor = descriptor("application/vnd.oci.image.layer.v1.tar+gzip", &layer);
    let platform_manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": config_descriptor,
        "layers": [layer_descriptor],
    });
    let platform_manifest_bytes =
        serde_json::to_vec(&platform_manifest).expect("manifest serializes");
    let platform_descriptor = descriptor(
        "application/vnd.oci.image.manifest.v1+json",
        &platform_manifest_bytes,
    );
    let platform_digest = platform_descriptor["digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let expected_subject = format!(
        "pkg:docker/{}@linux-amd64?platform=linux%2Famd64",
        image.image_name()
    );
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v0.1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": [{
            "name": expected_subject,
            "digest": { "sha256": platform_digest.trim_start_matches("sha256:") }
        }],
        "predicate": {
            "buildDefinition": {
                "buildType": "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
                "resolvedDependencies": [],
                "externalParameters": {
                    "configSource": { "path": "Dockerfile" },
                    "request": {
                        "frontend": "dockerfile.v0",
                        "locals": [{ "name": "context" }, { "name": "dockerfile" }]
                    }
                },
                "internalParameters": { "builderPlatform": "linux/amd64" }
            },
            "runDetails": {
                "builder": { "id": "" },
                "metadata": {
                    "buildkit_metadata": {
                        "vcs": {
                            "localdir:context": image.source_context(),
                            "localdir:dockerfile": image.source_context(),
                            "revision": SOURCE_SHA,
                            "source": "https://github.com/tailrocks/velnor-new"
                        }
                    }
                }
            }
        }
    });
    let statement_bytes = serde_json::to_vec(&statement).expect("statement serializes");
    let mut attestation_layer = descriptor("application/vnd.in-toto+json", &statement_bytes);
    attestation_layer["annotations"] = json!({
        "in-toto.io/predicate-type": "https://slsa.dev/provenance/v1"
    });
    let attestation_config = json!({
        "architecture": "unknown",
        "os": "unknown",
        "config": {},
        "rootfs": { "type": "layers", "diff_ids": [attestation_layer["digest"]] }
    });
    let attestation_config_bytes =
        serde_json::to_vec(&attestation_config).expect("attestation config serializes");
    let attestation_config_descriptor = descriptor(
        "application/vnd.oci.image.config.v1+json",
        &attestation_config_bytes,
    );
    let attestation_manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": attestation_config_descriptor,
        "layers": [attestation_layer.clone()],
    });
    let attestation_manifest_bytes =
        serde_json::to_vec(&attestation_manifest).expect("attestation manifest serializes");
    let attestation_descriptor = json!({
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "digest": sha256_prefixed(&attestation_manifest_bytes),
        "size": attestation_manifest_bytes.len(),
        "annotations": {
            "vnd.docker.reference.digest": platform_digest,
            "vnd.docker.reference.type": "attestation-manifest"
        },
        "platform": { "architecture": "unknown", "os": "unknown" }
    });
    let nested_index = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": platform_digest,
                "size": platform_manifest_bytes.len(),
                "platform": { "architecture": "amd64", "os": "linux" }
            },
            attestation_descriptor
        ]
    });
    let nested_index_bytes = serde_json::to_vec(&nested_index).expect("index serializes");
    let nested_index_descriptor = json!({
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "digest": sha256_prefixed(&nested_index_bytes),
        "size": nested_index_bytes.len(),
        "annotations": {
            "io.containerd.image.name": format!("docker.io/library/{}:linux-amd64", image.image_name()),
            "org.opencontainers.image.ref.name": "linux-amd64"
        }
    });
    let outer_index = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [nested_index_descriptor]
    });
    let outer_index_bytes = serde_json::to_vec(&outer_index).expect("outer index serializes");
    let legacy_manifest = json!([{
        "Config": blob_path(config_descriptor["digest"].as_str().expect("config digest")),
        "RepoTags": [image.release_tag()],
        "Layers": [blob_path(layer_descriptor["digest"].as_str().expect("layer digest"))]
    }]);
    let legacy_manifest_bytes = serde_json::to_vec(&legacy_manifest).expect("legacy serializes");
    let blobs = vec![
        (
            config_descriptor["digest"]
                .as_str()
                .expect("config digest")
                .to_owned(),
            image_config_bytes,
        ),
        (
            layer_descriptor["digest"]
                .as_str()
                .expect("layer digest")
                .to_owned(),
            layer.clone(),
        ),
        (
            sha256_prefixed(&platform_manifest_bytes),
            platform_manifest_bytes,
        ),
        (
            attestation_config_descriptor["digest"]
                .as_str()
                .expect("attestation config digest")
                .to_owned(),
            attestation_config_bytes,
        ),
        (
            attestation_layer["digest"]
                .as_str()
                .expect("attestation layer digest")
                .to_owned(),
            statement_bytes,
        ),
        (
            attestation_descriptor["digest"]
                .as_str()
                .expect("attestation manifest digest")
                .to_owned(),
            attestation_manifest_bytes,
        ),
        (sha256_prefixed(&nested_index_bytes), nested_index_bytes),
    ];
    let archive = build_tar(&outer_index_bytes, &legacy_manifest_bytes, &blobs);
    Fixture {
        archive,
        layer_blobs: blobs,
        expected_manifest: platform_digest,
    }
}

fn runtime_config(image: WorkerImage) -> Value {
    let mut config = json!({
        "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
        "Entrypoint": [image.entrypoint()],
        "Labels": {
            "org.opencontainers.image.created": "2026-10-06T21:25:30.040262+00:00",
            "org.opencontainers.image.description": "Ubuntu image description",
            "org.opencontainers.image.title": "ubuntu",
            "org.opencontainers.image.version": "26.04"
        }
    });
    if image == WorkerImage::Runner {
        config["User"] = json!("runner");
        config["WorkingDir"] = json!("/home/runner");
    }
    config
}

fn descriptor(media_type: &str, bytes: &[u8]) -> Value {
    json!({
        "mediaType": media_type,
        "digest": sha256_prefixed(bytes),
        "size": bytes.len()
    })
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256(bytes))
}

fn blob_path(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

fn build_tar(index: &[u8], legacy: &[u8], blobs: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    append_dir(&mut builder, "blobs");
    append_dir(&mut builder, "blobs/sha256");
    append_file(
        &mut builder,
        "oci-layout",
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    );
    append_file(&mut builder, "index.json", index);
    append_file(&mut builder, "manifest.json", legacy);
    for (digest, bytes) in blobs {
        append_file(&mut builder, &blob_path(digest), bytes);
    }
    builder.into_inner().expect("tar finalizes")
}

fn append_dir<W: std::io::Write>(builder: &mut Builder<W>, path: &str) {
    let mut header = new_header(EntryType::Directory, 0).expect("directory header");
    builder
        .append_data(&mut header, format!("{path}/"), Cursor::new([]))
        .expect("directory appended");
}

fn append_file<W: std::io::Write>(builder: &mut Builder<W>, path: &str, bytes: &[u8]) {
    let mut header = new_header(EntryType::Regular, bytes.len() as u64).expect("file header");
    builder
        .append_data(&mut header, path, bytes)
        .expect("file appended");
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-worker-archive-{label}-{}-{}",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_fixture(label: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_path(label);
    fs::write(&path, bytes).expect("fixture writes");
    path
}

#[test]
fn verifies_runner_and_dind_archive_chains() {
    for image in [WorkerImage::Runner, WorkerImage::Dind] {
        let fixture = fixture(image);
        let archive_path = write_fixture("verified", &fixture.archive);
        let expected_sha = sha256(&fixture.archive);
        let parsed = verify(
            &archive_path,
            image,
            SOURCE_SHA,
            &expected_sha,
            &image.release_tag(),
        )
        .expect("strict image archive verifies");
        assert_eq!(parsed.manifest_digest, fixture.expected_manifest);
        assert_eq!(parsed.blob_inventory.len(), fixture.layer_blobs.len());
        let _ = fs::remove_file(archive_path);
    }
}

#[test]
fn rejects_wrong_source_sha_and_archive_identity() {
    let fixture = fixture(WorkerImage::Runner);
    let archive_path = write_fixture("wrong-source", &fixture.archive);
    let expected_sha = sha256(&fixture.archive);
    assert!(
        verify(
            &archive_path,
            WorkerImage::Runner,
            &"b".repeat(40),
            &expected_sha,
            "velnor-runner:linux-amd64"
        )
        .is_err()
    );
    assert!(
        verify(
            &archive_path,
            WorkerImage::Runner,
            SOURCE_SHA,
            &"0".repeat(64),
            "velnor-runner:linux-amd64"
        )
        .is_err()
    );
    let _ = fs::remove_file(archive_path);
}

#[test]
fn derived_load_reference_rewrites_both_tag_surfaces_only() {
    let fixture = fixture(WorkerImage::Runner);
    let source_path = write_fixture("derived-source", &fixture.archive);
    let original = verify(
        &source_path,
        WorkerImage::Runner,
        SOURCE_SHA,
        &sha256(&fixture.archive),
        &WorkerImage::Runner.release_tag(),
    )
    .expect("original archive verifies");
    let output_path = temp_path("derived-output");
    let original_headers = header_identities(&source_path);
    let derived = derive_load_archive(
        &source_path,
        &output_path,
        WorkerImage::Runner,
        SOURCE_SHA,
        &original,
    )
    .expect("derived archive verifies");
    assert_eq!(header_identities(&output_path), original_headers);
    let expected_tag = format!("velnor-runner:v{SOURCE_SHA}-{}", original.archive_sha256);
    assert_eq!(derived.repo_tag, expected_tag);
    assert!(derived.archive_index_digest != original.archive_index_digest);
    assert!(derived.sha256 != original.archive_sha256);
    assert_eq!(
        derived.parsed.image_index_digest,
        original.image_index_digest
    );

    let mut derived_archive = tar::Archive::new(File::open(&derived.path).expect("derived opens"));
    let mut index = Vec::new();
    let mut legacy = Vec::new();
    for entry in derived_archive.entries().expect("entries read") {
        let mut entry = entry.expect("entry valid");
        match entry
            .path()
            .expect("entry path")
            .to_str()
            .expect("utf8 path")
        {
            "index.json" => {
                std::io::Read::read_to_end(&mut entry, &mut index).expect("index read");
            }
            "manifest.json" => {
                std::io::Read::read_to_end(&mut entry, &mut legacy).expect("legacy read");
            }
            _ => {}
        }
    }
    let index: Value = serde_json::from_slice(&index).expect("index json");
    let legacy: Value = serde_json::from_slice(&legacy).expect("legacy json");
    assert_eq!(
        index
            .pointer("/manifests/0/annotations/io.containerd.image.name")
            .and_then(Value::as_str),
        Some(format!("docker.io/library/{expected_tag}").as_str())
    );
    assert_eq!(
        index
            .pointer("/manifests/0/annotations/org.opencontainers.image.ref.name")
            .and_then(Value::as_str),
        expected_tag.split_once(':').map(|(_, tag)| tag)
    );
    assert_eq!(
        legacy[0]["RepoTags"][0].as_str(),
        Some(expected_tag.as_str())
    );
    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(output_path);
}

fn header_identities(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = tar::Archive::new(File::open(path).expect("archive opens"));
    archive
        .entries()
        .expect("entries read")
        .map(|entry| {
            let entry = entry.expect("entry valid");
            let path = entry
                .path()
                .expect("entry path")
                .to_str()
                .expect("entry path is utf8")
                .to_owned();
            let mut header = entry.header().as_bytes().to_vec();
            header[124..136].fill(0);
            header[148..156].fill(0);
            (path, header)
        })
        .collect()
}

#[test]
fn rejects_a_tag_with_an_unowned_image_name() {
    let fixture = fixture(WorkerImage::Runner);
    let archive_path = write_fixture("wrong-tag", &fixture.archive);
    assert!(
        verify(
            &archive_path,
            WorkerImage::Runner,
            SOURCE_SHA,
            &sha256(&fixture.archive),
            "some-other-image:linux-amd64"
        )
        .is_err()
    );
    let _ = fs::remove_file(archive_path);
}
