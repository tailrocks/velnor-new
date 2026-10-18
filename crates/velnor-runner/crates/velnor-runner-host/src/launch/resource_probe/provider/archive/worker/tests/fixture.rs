use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tar::{Builder, EntryType, Header};

use super::super::{WorkerImage, layout::sha256};

pub(super) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

pub(super) struct Fixture {
    pub(super) archive: Vec<u8>,
    pub(super) layer_blobs: Vec<(String, Vec<u8>)>,
    pub(super) expected_manifest: String,
}

struct PlatformParts {
    image_config_bytes: Vec<u8>,
    config_descriptor: Value,
    layer: Vec<u8>,
    layer_descriptor: Value,
    manifest_bytes: Vec<u8>,
    manifest_digest: String,
}

struct AttestationParts {
    config_bytes: Vec<u8>,
    config_descriptor: Value,
    layer_bytes: Vec<u8>,
    layer_descriptor: Value,
    manifest_bytes: Vec<u8>,
    manifest_descriptor: Value,
}

pub(super) fn fixture(image: WorkerImage) -> Fixture {
    fixture_with_config(image, |_| {})
}

pub(super) fn fixture_with_config(image: WorkerImage, mutate: impl FnOnce(&mut Value)) -> Fixture {
    let platform = platform_parts(image, mutate);
    let statement = build_statement(image, &platform.manifest_digest);
    let attestation = attestation_parts(statement, &platform.manifest_digest);
    let nested_index = nested_index(
        &platform.manifest_digest,
        &platform.manifest_bytes,
        &attestation,
    );
    let nested_index_descriptor =
        descriptor("application/vnd.oci.image.index.v1+json", &nested_index);
    let outer_index = outer_index(image, nested_index_descriptor);
    let legacy_manifest = legacy_manifest(
        image,
        &platform.config_descriptor,
        &platform.layer_descriptor,
    );
    let blobs = fixture_blobs(&platform, &attestation, &nested_index);
    Fixture {
        archive: build_tar(&outer_index, &legacy_manifest, &blobs),
        layer_blobs: blobs,
        expected_manifest: platform.manifest_digest,
    }
}

fn platform_parts(image: WorkerImage, mutate: impl FnOnce(&mut Value)) -> PlatformParts {
    let layer = b"fixture compressed layer".to_vec();
    let config = runtime_config(image);
    let mut image_config = json!({
        "architecture": "amd64",
        "os": "linux",
        "config": config,
        "rootfs": { "type": "layers", "diff_ids": [format!("sha256:{}", "1".repeat(64))] }
    });
    mutate(&mut image_config);
    let image_config_bytes = serde_json::to_vec(&image_config).expect("image config JSON is valid");
    let config_descriptor = descriptor(
        "application/vnd.oci.image.config.v1+json",
        &image_config_bytes,
    );
    let layer_descriptor = descriptor("application/vnd.oci.image.layer.v1.tar+gzip", &layer);
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": config_descriptor,
        "layers": [layer_descriptor],
    });
    let manifest_bytes = serde_json::to_vec(&manifest).expect("platform manifest JSON is valid");
    let manifest_digest = descriptor_digest(&manifest_bytes);
    PlatformParts {
        image_config_bytes,
        config_descriptor,
        layer,
        layer_descriptor,
        manifest_bytes,
        manifest_digest,
    }
}

fn build_statement(image: WorkerImage, manifest_digest: &str) -> Vec<u8> {
    let expected_subject = format!(
        "pkg:docker/{}@linux-amd64?platform=linux%2Famd64",
        image.image_name()
    );
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v0.1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": [{
            "name": expected_subject,
            "digest": { "sha256": manifest_digest.trim_start_matches("sha256:") }
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
    serde_json::to_vec(&statement).expect("attestation statement JSON is valid")
}

fn attestation_parts(statement: Vec<u8>, manifest_digest: &str) -> AttestationParts {
    let mut layer_descriptor = descriptor("application/vnd.in-toto+json", &statement);
    layer_descriptor["annotations"] = json!({
        "in-toto.io/predicate-type": "https://slsa.dev/provenance/v1"
    });
    let config = json!({
        "architecture": "unknown",
        "os": "unknown",
        "config": {},
        "rootfs": { "type": "layers", "diff_ids": [layer_descriptor["digest"]] }
    });
    let config_bytes = serde_json::to_vec(&config).expect("attestation config JSON is valid");
    let config_descriptor = descriptor("application/vnd.oci.image.config.v1+json", &config_bytes);
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": config_descriptor,
        "layers": [layer_descriptor.clone()],
    });
    let manifest_bytes = serde_json::to_vec(&manifest).expect("attestation manifest JSON is valid");
    let manifest_descriptor = json!({
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "digest": descriptor_digest(&manifest_bytes),
        "size": manifest_bytes.len(),
        "annotations": {
            "vnd.docker.reference.digest": manifest_digest,
            "vnd.docker.reference.type": "attestation-manifest"
        },
        "platform": { "architecture": "unknown", "os": "unknown" }
    });
    AttestationParts {
        config_bytes,
        config_descriptor,
        layer_bytes: statement,
        layer_descriptor,
        manifest_bytes,
        manifest_descriptor,
    }
}

fn nested_index(
    manifest_digest: &str,
    manifest_bytes: &[u8],
    attestation: &AttestationParts,
) -> Vec<u8> {
    let value = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": manifest_digest,
                "size": manifest_bytes.len(),
                "platform": { "architecture": "amd64", "os": "linux" }
            },
            attestation.manifest_descriptor
        ]
    });
    serde_json::to_vec(&value).expect("nested image index JSON is valid")
}

fn outer_index(image: WorkerImage, nested_descriptor: Value) -> Vec<u8> {
    let mut nested_descriptor = nested_descriptor;
    nested_descriptor["annotations"] = json!({
        "io.containerd.image.name": format!("docker.io/library/{}:linux-amd64", image.image_name()),
        "org.opencontainers.image.ref.name": "linux-amd64"
    });
    let value = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [nested_descriptor]
    });
    serde_json::to_vec(&value).expect("outer image index JSON is valid")
}

fn legacy_manifest(image: WorkerImage, config: &Value, layer: &Value) -> Vec<u8> {
    let value = json!([{
        "Config": blob_path(config["digest"].as_str().expect("config digest")),
        "RepoTags": [image.release_tag()],
        "Layers": [blob_path(layer["digest"].as_str().expect("layer digest"))]
    }]);
    serde_json::to_vec(&value).expect("legacy manifest JSON is valid")
}

fn fixture_blobs(
    platform: &PlatformParts,
    attestation: &AttestationParts,
    nested_index: &[u8],
) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            value_digest(&platform.config_descriptor),
            platform.image_config_bytes.clone(),
        ),
        (
            value_digest(&platform.layer_descriptor),
            platform.layer.clone(),
        ),
        (
            descriptor_digest(&platform.manifest_bytes),
            platform.manifest_bytes.clone(),
        ),
        (
            value_digest(&attestation.config_descriptor),
            attestation.config_bytes.clone(),
        ),
        (
            value_digest(&attestation.layer_descriptor),
            attestation.layer_bytes.clone(),
        ),
        (
            value_digest(&attestation.manifest_descriptor),
            attestation.manifest_bytes.clone(),
        ),
        (descriptor_digest(nested_index), nested_index.to_vec()),
    ]
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
    json!({ "mediaType": media_type, "digest": descriptor_digest(bytes), "size": bytes.len() })
}

fn descriptor_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256(bytes))
}

fn value_digest(value: &Value) -> String {
    value["digest"]
        .as_str()
        .expect("descriptor digest")
        .to_owned()
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
    builder.into_inner().expect("tar archive finalizes")
}

fn append_dir<W: std::io::Write>(builder: &mut Builder<W>, path: &str) {
    let mut header = new_header(EntryType::Directory, 0);
    builder
        .append_data(&mut header, format!("{path}/"), Cursor::new([]))
        .expect("directory is written");
}

fn append_file<W: std::io::Write>(builder: &mut Builder<W>, path: &str, bytes: &[u8]) {
    let mut header = new_header(EntryType::Regular, bytes.len() as u64);
    builder
        .append_data(&mut header, path, bytes)
        .expect("file is written");
}

fn new_header(kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(if kind == EntryType::Directory {
        0o755
    } else {
        0o444
    });
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    header
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-worker-archive-{label}-{}-{}",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(super) fn write_fixture(label: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_path(label);
    fs::write(&path, bytes).expect("fixture bytes are written");
    path
}

mod archive_mutations;

pub(super) use archive_mutations::{append_file_entry, append_pax_link, rewrite_json_entry};
