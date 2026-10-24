use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use sha2::{Digest, Sha256};
use tar::{Builder, Header};

use super::{inspect_oci_candidate, json::parse};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn strict_json_rejects_duplicate_keys_and_trailing_data() {
    assert!(parse(br#"{"schemaVersion":2,"schemaVersion":2}"#).is_err());
    assert!(parse(br#"{"schemaVersion":2} false"#).is_err());
}

#[test]
fn valid_graph_accepts_annotated_layer_descriptor_and_authenticates_archive() {
    let temporary = FixtureDir::new();
    let path = temporary.path().join("valid.oci.tar");
    let expected = fixture(&path, Mutation::AnnotatedLayer);
    let evidence = inspect_oci_candidate(&path).expect("valid OCI candidate");
    assert_eq!(
        evidence.platform_manifest_digest,
        format!("sha256:{}", expected.manifest)
    );
    assert_eq!(
        evidence.image_configuration_digest,
        format!("sha256:{}", expected.config)
    );
    assert_eq!(
        evidence.layer_digests,
        [format!("sha256:{}", expected.layer)]
    );
    assert_eq!(
        evidence.archive_size_bytes,
        std::fs::metadata(&path).expect("archive stat").len()
    );
    assert_eq!(evidence.archive_sha256, file_sha256(&path));
}

#[test]
fn graph_rejects_each_isolated_blob_and_diff_id_mutation() {
    for (mutation, expected) in [
        (Mutation::UnreferencedBlob, "oci_unreferenced_blob"),
        (Mutation::MissingLayer, "oci_blob_missing"),
        (Mutation::ChangedLayer, "oci_blob_digest"),
        (Mutation::WrongLayerSize, "oci_blob_size"),
        (Mutation::WrongDiffId, "oci_diff_id_mismatch"),
        (Mutation::ConflictingDescriptor, "oci_descriptor_conflict"),
    ] {
        let temporary = FixtureDir::new();
        let path = temporary.path().join("invalid.oci.tar");
        fixture(&path, mutation);
        let error = inspect_oci_candidate(&path).expect_err("isolated mutation rejected");
        assert_eq!(error.to_string(), expected, "{mutation:?}");
    }
}

#[test]
fn raw_archive_scan_rejects_unsafe_member_path_before_tar_parser() {
    let temporary = FixtureDir::new();
    let path = temporary.path().join("unsafe-path.oci.tar");
    let mut archive = Builder::new(std::fs::File::create(&path).expect("archive file"));
    append(
        &mut archive,
        "oci-layout",
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    );
    append(
        &mut archive,
        "index.json",
        br#"{"schemaVersion":2,"manifests":[]}"#,
    );
    append_raw_path(&mut archive, b"../bad", b"unsafe");
    archive.finish().expect("finish archive");
    let error = inspect_oci_candidate(&path).expect_err("unsafe member rejected");
    assert_eq!(error.to_string(), "archive_path_unsafe");
}

#[derive(Debug, Clone, Copy)]
enum Mutation {
    AnnotatedLayer,
    UnreferencedBlob,
    MissingLayer,
    ChangedLayer,
    WrongLayerSize,
    WrongDiffId,
    ConflictingDescriptor,
}

struct Digests {
    manifest: String,
    config: String,
    layer: String,
}

fn fixture(path: &Path, mutation: Mutation) -> Digests {
    let raw_layer = empty_tar();
    let mut layer = gzip(&raw_layer);
    let mut diff_ids = vec![format!("sha256:{}", hex(&Sha256::digest(&raw_layer)))];
    if matches!(mutation, Mutation::WrongDiffId) {
        diff_ids[0] = format!("sha256:{}", "a".repeat(64));
    }
    if matches!(mutation, Mutation::ConflictingDescriptor) {
        diff_ids.push(diff_ids[0].clone());
    }
    let config = serde_json::to_vec(&json!({
        "architecture":"amd64", "os":"linux",
        "rootfs":{"type":"layers", "diff_ids":diff_ids}
    }))
    .expect("config json");
    let config_digest = hex(&Sha256::digest(&config));
    let layer_digest = hex(&Sha256::digest(&layer));
    let mut first_layer = json!({
        "mediaType":"application/vnd.oci.image.layer.v1.tar+gzip",
        "digest":format!("sha256:{layer_digest}"), "size":layer.len()
    });
    if matches!(mutation, Mutation::AnnotatedLayer) {
        first_layer["annotations"] = json!({"org.opencontainers.image.title":"rootfs"});
        first_layer["futureDescriptorField"] = json!("legacy-compatible");
    }
    if matches!(mutation, Mutation::WrongLayerSize) {
        first_layer["size"] = json!(layer.len() + 1);
    }
    let layers = if matches!(mutation, Mutation::ConflictingDescriptor) {
        vec![
            first_layer,
            json!({
                "mediaType":"application/vnd.oci.image.layer.v1.tar",
                "digest":format!("sha256:{layer_digest}"), "size":layer.len()
            }),
        ]
    } else {
        vec![first_layer]
    };
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion":2, "mediaType":"application/vnd.oci.image.manifest.v1+json",
        "config":{"mediaType":"application/vnd.oci.image.config.v1+json",
            "digest":format!("sha256:{config_digest}"), "size":config.len(),
            "legacyExtra":"accepted"},
        "layers":layers
    }))
    .expect("manifest json");
    let manifest_digest = hex(&Sha256::digest(&manifest));
    let index = serde_json::to_vec(&json!({
        "schemaVersion":2, "mediaType":"application/vnd.oci.image.index.v1+json",
        "manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json",
            "digest":format!("sha256:{manifest_digest}"), "size":manifest.len(),
            "platform":{"os":"linux", "architecture":"amd64"}}]
    }))
    .expect("index json");
    if matches!(mutation, Mutation::ChangedLayer) {
        layer[4] ^= 1;
    }
    write_fixture_archive(
        path,
        &FixtureArchive {
            index: &index,
            manifest: &manifest,
            config: &config,
            layer: &layer,
            layer_hash: &layer_digest,
            mutation,
        },
    );
    Digests {
        manifest: manifest_digest,
        config: config_digest,
        layer: layer_digest,
    }
}

struct FixtureArchive<'a> {
    index: &'a [u8],
    manifest: &'a [u8],
    config: &'a [u8],
    layer: &'a [u8],
    layer_hash: &'a str,
    mutation: Mutation,
}

fn write_fixture_archive(path: &Path, fixture: &FixtureArchive<'_>) {
    let mut archive = Builder::new(std::fs::File::create(path).expect("archive file"));
    append(
        &mut archive,
        "oci-layout",
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    );
    append(&mut archive, "index.json", fixture.index);
    append(
        &mut archive,
        &format!("blobs/sha256/{}", hex(&Sha256::digest(fixture.manifest))),
        fixture.manifest,
    );
    append(
        &mut archive,
        &format!("blobs/sha256/{}", hex(&Sha256::digest(fixture.config))),
        fixture.config,
    );
    if !matches!(fixture.mutation, Mutation::MissingLayer) {
        append(
            &mut archive,
            &format!("blobs/sha256/{}", fixture.layer_hash),
            fixture.layer,
        );
    }
    if matches!(fixture.mutation, Mutation::UnreferencedBlob) {
        let unused = b"unreferenced";
        append(
            &mut archive,
            &format!("blobs/sha256/{}", hex(&Sha256::digest(unused))),
            unused,
        );
    }
    archive.finish().expect("finish archive");
}

fn append(archive: &mut Builder<std::fs::File>, name: &str, bytes: &[u8]) {
    let mut header = Header::new_gnu();
    header.set_path(name).expect("archive path");
    append_header(archive, header, bytes);
}

fn append_raw_path(archive: &mut Builder<std::fs::File>, name: &[u8], bytes: &[u8]) {
    let mut header = Header::new_gnu();
    header.as_mut_bytes()[..name.len()].copy_from_slice(name);
    append_header(archive, header, bytes);
}

fn append_header(archive: &mut Builder<std::fs::File>, mut header: Header, bytes: &[u8]) {
    header.set_size(u64::try_from(bytes.len()).expect("fixture size"));
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, bytes).expect("append entry");
}

fn empty_tar() -> Vec<u8> {
    vec![0; 1024]
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(raw).expect("gzip layer");
    gzip.finish().expect("finish gzip")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn file_sha256(path: &Path) -> String {
    hex(&Sha256::digest(std::fs::read(path).expect("archive bytes")))
}

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-oci-fixture-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).expect("create fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}
