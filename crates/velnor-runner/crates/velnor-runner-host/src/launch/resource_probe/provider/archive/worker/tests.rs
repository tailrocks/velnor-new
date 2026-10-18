use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use super::layout::sha256;
use super::{WorkerImage, derive_load_archive, verify};
use fixture::{
    SOURCE_SHA, append_file_entry, append_pax_link, fixture, fixture_with_config,
    rewrite_json_entry, write_fixture,
};

mod fixture;

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

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
        .expect("strict image archive passes identity checks");
        assert_eq!(parsed.manifest_digest, fixture.expected_manifest);
        assert_eq!(parsed.blob_inventory_len(), fixture.layer_blobs.len());
        fs::remove_file(archive_path).expect("verified archive is removed");
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
    fs::remove_file(archive_path).expect("wrong-source archive is removed");
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
    .expect("original archive passes identity checks");
    let output_path = temp_path("derived-output");
    let original_headers = header_identities(&source_path);
    let derived = derive_load_archive(
        &source_path,
        &output_path,
        WorkerImage::Runner,
        SOURCE_SHA,
        &original,
    )
    .expect("derived archive passes identity checks");
    assert_eq!(header_identities(&output_path), original_headers);
    let expected_tag = format!("velnor-runner:v{SOURCE_SHA}-{}", original.archive_sha256);
    assert_eq!(derived.repo_tag, expected_tag);
    assert_ne!(derived.archive_index_digest, original.archive_index_digest);
    assert_ne!(derived.sha256, original.archive_sha256);
    assert_eq!(
        derived.parsed.image_index_digest,
        original.image_index_digest
    );

    let mut derived_archive = tar::Archive::new(File::open(&derived.path).expect("derived opens"));
    let (index, legacy) = rewritten_tag_surfaces(&mut derived_archive);
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
    fs::remove_file(source_path).expect("source archive is removed");
    fs::remove_file(output_path).expect("derived archive is removed");
}

fn rewritten_tag_surfaces<R: std::io::Read>(archive: &mut tar::Archive<R>) -> (Value, Value) {
    let mut index = Vec::new();
    let mut legacy = Vec::new();
    for entry in archive.entries().expect("entries read") {
        let mut entry = entry.expect("entry valid");
        match entry
            .path()
            .expect("entry path")
            .to_str()
            .expect("utf8 path")
        {
            "index.json" => {
                std::io::Read::read_to_end(&mut entry, &mut index).expect("index bytes read");
            }
            "manifest.json" => {
                std::io::Read::read_to_end(&mut entry, &mut legacy).expect("manifest bytes read");
            }
            _ => {}
        }
    }
    (
        serde_json::from_slice(&index).expect("index JSON is valid"),
        serde_json::from_slice(&legacy).expect("legacy manifest JSON is valid"),
    )
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
    fs::remove_file(archive_path).expect("wrong-tag archive is removed");
}

#[test]
fn rejects_traversal_duplicate_and_pax_link_entries() {
    let fixture = fixture(WorkerImage::Runner);
    let traversal = append_file_entry(&fixture.archive, "../outside", b"foreign");
    assert_archive_rejected("traversal", &traversal, WorkerImage::Runner);

    let duplicate = append_file_entry(&fixture.archive, "index.json", b"{}");
    assert_archive_rejected("duplicate", &duplicate, WorkerImage::Runner);

    let link = append_pax_link(&fixture.archive, "blobs/sha256/linked", "../../outside");
    assert_archive_rejected("pax-link", &link, WorkerImage::Runner);
}

#[test]
fn rejects_descriptor_digest_and_size_mismatches() {
    let fixture = fixture(WorkerImage::Runner);
    let wrong_digest = rewrite_json_entry(&fixture.archive, "index.json", |index| {
        index["manifests"][0]["digest"] = format!("sha256:{}", "0".repeat(64)).into();
    });
    assert_archive_rejected("descriptor-digest", &wrong_digest, WorkerImage::Runner);

    let wrong_size = rewrite_json_entry(&fixture.archive, "index.json", |index| {
        let size = index["manifests"][0]["size"]
            .as_u64()
            .expect("descriptor size is numeric");
        index["manifests"][0]["size"] = (size + 1).into();
    });
    assert_archive_rejected("descriptor-size", &wrong_size, WorkerImage::Runner);
}

#[test]
fn rejects_wrong_platform_config_and_rootfs() {
    for (label, mutate) in [
        (
            "architecture",
            Box::new(|config: &mut Value| config["architecture"] = "arm64".into())
                as Box<dyn FnOnce(&mut Value)>,
        ),
        (
            "operating-system",
            Box::new(|config: &mut Value| config["os"] = "windows".into()),
        ),
        (
            "rootfs-type",
            Box::new(|config: &mut Value| config["rootfs"]["type"] = "squashfs".into()),
        ),
        (
            "rootfs-diff-ids",
            Box::new(|config: &mut Value| config["rootfs"]["diff_ids"] = json!([])),
        ),
        (
            "runtime-config",
            Box::new(|config: &mut Value| {
                config["config"]["Entrypoint"] = json!(["/untrusted"]);
            }),
        ),
    ] {
        let fixture = fixture_with_config(WorkerImage::Runner, mutate);
        assert_archive_rejected(label, &fixture.archive, WorkerImage::Runner);
    }
}

#[test]
fn rejects_worker_archive_larger_than_the_bound_before_reading() {
    let path = temp_path("oversized-sparse");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("sparse fixture file is created")
        .set_len(super::model::MAX_WORKER_ARCHIVE_BYTES + 1)
        .expect("sparse fixture exceeds the archive bound");
    assert!(
        verify(
            &path,
            WorkerImage::Runner,
            SOURCE_SHA,
            &"0".repeat(64),
            &WorkerImage::Runner.release_tag(),
        )
        .is_err()
    );
    fs::remove_file(path).expect("oversized fixture is removed");
}

fn assert_archive_rejected(label: &str, bytes: &[u8], image: WorkerImage) {
    let path = write_fixture(label, bytes);
    assert!(
        verify(
            &path,
            image,
            SOURCE_SHA,
            &sha256(bytes),
            &image.release_tag(),
        )
        .is_err(),
        "malformed {label} archive is rejected"
    );
    fs::remove_file(path).expect("malformed archive is removed");
}

fn temp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-worker-archive-{label}-{}-{}",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}
