use super::*;
use std::{
    fs,
    io::{Seek, SeekFrom},
    time::Duration,
};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(5)).expect("deadline")
}

#[test]
fn preview_profiles_compose_full_assets_metadata_and_compression_without_changing_recovery() {
    use crate::package_release_artifacts::{
        MAX_HANDOFF_BYTES, MAX_PAYLOAD_BYTES, MAX_SBOM_BYTES, MAX_TOTAL_BYTES,
    };
    assert_eq!(ZipProfile::Recovery.compressed(), 1024 * 1024 * 1024);
    assert_eq!(ZipProfile::Recovery.unpacked(), 4 * 1024 * 1024 * 1024);
    assert_eq!(
        ZipProfile::PreviewLeaf.unpacked(),
        MAX_PAYLOAD_BYTES + MAX_SBOM_BYTES + NATIVE_MANIFEST_BYTES
    );
    assert_eq!(
        ZipProfile::PreviewAggregate.unpacked(),
        MAX_TOTAL_BYTES + MAX_HANDOFF_BYTES + NATIVE_MANIFEST_BYTES
    );
    for profile in [ZipProfile::PreviewLeaf, ZipProfile::PreviewAggregate] {
        assert!(
            profile.compressed() > profile.unpacked(),
            "incompressible bytes and framing fit"
        );
    }
    let two_payloads = 2 * 600 * 1024 * 1024;
    assert!(two_payloads > ZipProfile::Recovery.compressed());
    assert!(two_payloads < leaf_union_compressed_bound(2));
    assert!(two_payloads < ZipProfile::PreviewAggregate.unpacked());
    assert!(ZipProfile::PreviewAggregate.compressed() < 9 * 1024 * 1024 * 1024);
    assert_eq!(ZipProfile::Recovery.entries(), 4096);
    assert_eq!(ZipProfile::PreviewLeaf.entries(), 3);
    assert_eq!(
        ZipProfile::PreviewAggregate.entries(),
        crate::package_release_artifacts::MAX_FILE_COUNT + 1
    );
}

#[test]
fn held_file_zip_uses_the_same_closed_inventory_crc_and_identity_checks() {
    let root = tempfile::tempdir().expect("root");
    let archive = root.path().join("archive.zip");
    fs::write(&archive, tests::bytes()).expect("archive");
    let mut file = fs::File::open(&archive).expect("held archive");
    assert_eq!(
        read_manifest_file(
            &mut file,
            ZipProfile::PreviewAggregate,
            "proof.json",
            1024,
            deadline()
        )
        .expect("manifest"),
        b"native-proof"
    );
    let output = root.path().join("output");
    fs::create_dir(&output).expect("output");
    extract_file(
        &mut file,
        ZipProfile::PreviewAggregate,
        &output,
        &tests::expected(),
        deadline(),
    )
    .expect("stream extraction");
    assert_eq!(
        fs::read(output.join("payload")).expect("payload"),
        b"payload-body"
    );
    assert_eq!(fs::read_dir(&output).expect("closed inventory").count(), 2);
    let rejected = root.path().join("rejected");
    fs::create_dir(&rejected).expect("rejected");
    let mut expected = tests::expected();
    expected[1].sha256 = Some("0".repeat(64));
    assert!(
        extract_file(
            &mut file,
            ZipProfile::PreviewAggregate,
            &rejected,
            &expected,
            deadline()
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&rejected).expect("rollback").count(), 0);
}

#[test]
fn large_sparse_disk_input_is_rejected_at_the_closed_compressed_ceiling_without_reading_body() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("sparse.zip");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("owned sparse file");
    for profile in [
        ZipProfile::Recovery,
        ZipProfile::PreviewLeaf,
        ZipProfile::PreviewAggregate,
    ] {
        file.set_len(profile.compressed() + 1)
            .expect("sparse size only");
        file.seek(SeekFrom::Start(0)).expect("rewind");
        let error = read_manifest_file(&mut file, profile, "proof.json", 1024, deadline())
            .expect_err("compressed cap");
        assert!(error.to_string().contains("compressed_limit"), "{error}");
    }
}
