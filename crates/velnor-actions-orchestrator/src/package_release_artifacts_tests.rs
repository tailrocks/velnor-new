//! Focused admission cases for untrusted producer outputs.

use std::{fs, os::unix::fs::symlink, path::Path};

use super::*;
use sha2::{Digest, Sha256};
use support::{
    append_nonzero_tar_trailer, append_second_gzip_member, corrupt_gzip_crc, expected, fixture,
    truncate_gzip_trailer, write_archive, write_archive_with_mode, write_gnu_longname_archive,
    write_manifest, write_pax_archive, write_raw_source_files, write_sbom, write_source_files,
    write_symlink_archive,
};
use velnor_actions_contract::package_release::PackageReleaseAssetKind;

#[path = "package_release_artifacts_test_support.rs"]
mod support;

#[test]
fn accepts_exact_source_bound_inventory_and_stages_archives_without_extracting() {
    let fixture = fixture();
    write_source_files(&fixture, "x86_64", "1.2.3-preview.7+abcdef0");
    let verified = verify_package_output(
        &fixture.producer,
        "package-output.json",
        &fixture.staging,
        &fixture.products,
        &fixture.companions,
        PackageOutputIdentity {
            source_commit: &fixture.commit,
            source_ref: &fixture.source_ref,
            version: &fixture.version,
        },
    )
    .expect("valid producer output");
    assert_eq!(verified.source_commit, fixture.commit);
    assert_eq!(verified.source_ref, fixture.source_ref);
    assert_eq!(verified.version, fixture.version);
    assert_eq!(verified.artifacts.len(), 3);
    assert_eq!(
        verified.artifacts[0].release_asset.kind,
        PackageReleaseAssetKind::Companion
    );
    assert_eq!(
        verified.artifacts[1].release_asset.kind,
        PackageReleaseAssetKind::Payload
    );
    assert!(
        verified
            .artifacts
            .iter()
            .all(|artifact| artifact.path.is_file())
    );
    assert!(
        !fixture.staging.join("cli").exists(),
        "archives are never extracted"
    );
    let checksum_assets = verified
        .write_checksum_assets()
        .expect("publisher-generated checksums");
    assert_eq!(checksum_assets.len(), 2);
    assert_eq!(
        checksum_assets
            .iter()
            .find(|asset| asset.name == "SHA256SUMS")
            .expect("checksum list")
            .release_asset
            .kind,
        PackageReleaseAssetKind::ChecksumList
    );
}

#[test]
fn accepts_stable_raw_binary_payload_without_repacking() {
    let mut fixture = fixture();
    fixture.products[0].format =
        velnor_actions_contract::package_release::PackageReleasePayloadFormat::RawBinary;
    write_raw_source_files(&fixture);
    let verified = verify_package_output(
        &fixture.producer,
        "package-output.json",
        &fixture.staging,
        &fixture.products,
        &fixture.companions,
        PackageOutputIdentity {
            source_commit: &fixture.commit,
            source_ref: &fixture.source_ref,
            version: &fixture.version,
        },
    )
    .expect("raw binary payload passes format and target admission");
    let payload = verified
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "cli-x86_64-unknown-linux-gnu")
        .expect("original raw payload name retained");
    assert_eq!(payload.release_asset.kind, PackageReleaseAssetKind::Payload);
}

#[test]
fn rejects_wrong_handoff_source_ref_or_version() {
    let fixture = fixture();
    write_source_files(&fixture, "x86_64", &fixture.version);
    let mut value: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.producer.join("package-output.json")).expect("read handoff"),
    )
    .expect("handoff JSON");
    value["source_commit"] = serde_json::json!("b".repeat(40));
    write_manifest(&fixture, &value);
    assert!(verify(&fixture).is_err());
}

#[test]
fn rejects_unexpected_or_missing_output_files() {
    let extra_fixture = fixture();
    write_source_files(&extra_fixture, "x86_64", &extra_fixture.version);
    fs::write(extra_fixture.producer.join("extra.txt"), b"extra").expect("extra file");
    assert!(verify(&extra_fixture).is_err());

    let missing_fixture = fixture();
    write_source_files(&missing_fixture, "x86_64", &missing_fixture.version);
    fs::remove_file(
        missing_fixture
            .producer
            .join(&missing_fixture.companions[0].path),
    )
    .expect("remove companion");
    assert!(verify(&missing_fixture).is_err());
}

#[test]
fn rejects_symlink_output_and_path_escape() {
    let fixture = fixture();
    write_source_files(&fixture, "x86_64", &fixture.version);
    fs::remove_file(fixture.producer.join(&fixture.companions[0].path)).expect("remove companion");
    symlink(
        "/etc/passwd",
        fixture.producer.join(&fixture.companions[0].path),
    )
    .expect("symlink companion");
    assert!(verify(&fixture).is_err());
    assert!(validate_relative_path("../outside").is_err());
    assert!(validate_relative_path("/absolute").is_err());
}

#[test]
fn rejects_archive_member_traversal_duplicates_and_wrong_architecture() {
    for entries in [
        vec![("../escape", "x86_64")],
        vec![("cli", "x86_64"), ("cli", "x86_64")],
        vec![("cli", "aarch64")],
    ] {
        let fixture = fixture();
        write_source_files(&fixture, "x86_64", &fixture.version);
        // Rewrite the archive after the helper generated the remaining files.
        write_archive(
            &fixture.producer.join(expected().payload_name),
            &entries,
            true,
        );
        rewrite_manifest_digests(&fixture);
        assert!(verify(&fixture).is_err());
    }
}

#[test]
fn rejects_archive_symlink_members_without_extracting_them() {
    let fixture = fixture();
    write_source_files(&fixture, "x86_64", &fixture.version);
    write_symlink_archive(&fixture.producer.join(expected().payload_name));
    rewrite_manifest_digests(&fixture);
    assert!(verify(&fixture).is_err());
    assert!(!fixture.producer.join("outside").exists());
}

#[test]
fn rejects_incomplete_or_ambiguous_gzip_and_tar_containers() {
    let mutations: [fn(&Path); 4] = [
        append_nonzero_tar_trailer,
        append_second_gzip_member,
        corrupt_gzip_crc,
        truncate_gzip_trailer,
    ];
    for mutate in mutations {
        let fixture = fixture();
        write_source_files(&fixture, "x86_64", &fixture.version);
        let archive = fixture.producer.join(expected().payload_name);
        mutate(&archive);
        rewrite_manifest_digests(&fixture);
        assert!(verify(&fixture).is_err(), "must reject {mutate:?}");
    }
}

#[test]
fn rejects_pax_and_gnu_metadata_headers_and_noncanonical_executable_modes() {
    for write_archive in [write_pax_archive, write_gnu_longname_archive] {
        let fixture = fixture();
        write_source_files(&fixture, "x86_64", &fixture.version);
        write_archive(&fixture.producer.join(expected().payload_name));
        rewrite_manifest_digests(&fixture);
        assert!(verify(&fixture).is_err());
    }
    for mode in [0o555, 0o775, 0o4755] {
        let fixture = fixture();
        write_source_files(&fixture, "x86_64", &fixture.version);
        write_archive_with_mode(&fixture.producer.join(expected().payload_name), mode);
        rewrite_manifest_digests(&fixture);
        assert!(verify(&fixture).is_err(), "must reject mode {mode:o}");
    }
}

#[test]
fn rejects_sbom_bound_to_another_archive_or_digest() {
    let fixture = fixture();
    write_source_files(&fixture, "x86_64", &fixture.version);
    write_sbom(
        &fixture.producer.join(expected().sbom_name),
        "other.tar.gz",
        &"a".repeat(64),
    );
    rewrite_manifest_digests(&fixture);
    assert!(verify(&fixture).is_err());
}

#[test]
fn optional_companion_source_identity_fields_must_match_when_present() {
    let mut fixture = fixture();
    fixture.companions[0].path = "capsule-manifest.json".to_owned();
    fixture.companions[0].asset_name = "capsule-manifest.json".to_owned();
    write_source_files(&fixture, "x86_64", &fixture.version);
    fs::write(
        fixture.producer.join(&fixture.companions[0].path),
        serde_json::json!({"version":"9.9.9"}).to_string(),
    )
    .expect("write mismatched companion identity");
    rewrite_manifest_digests(&fixture);
    assert!(verify(&fixture).is_err());
}

fn verify(fixture: &support::Fixture) -> Result<VerifiedPackageOutput, OrchestratorError> {
    verify_package_output(
        &fixture.producer,
        "package-output.json",
        &fixture.staging,
        &fixture.products,
        &fixture.companions,
        PackageOutputIdentity {
            source_commit: &fixture.commit,
            source_ref: &fixture.source_ref,
            version: &fixture.version,
        },
    )
}

fn rewrite_manifest_digests(fixture: &support::Fixture) {
    let archive_path = fixture.producer.join(expected().payload_name);
    let sbom_path = fixture.producer.join(expected().sbom_name);
    let companion_path = fixture.producer.join(&fixture.companions[0].path);
    let mut value: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.producer.join("package-output.json")).expect("read handoff"),
    )
    .expect("handoff JSON");
    value["payloads"][0]["sha256"] = serde_json::json!(sha256_file(&archive_path));
    value["companions"][0]["sha256"] = serde_json::json!(sha256_file(&companion_path));
    value["sboms"][0]["sha256"] = serde_json::json!(sha256_file(&sbom_path));
    write_manifest(fixture, &value);
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read file for digest");
    let digest = Sha256::digest(bytes);
    let mut output = String::new();
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("format digest");
    }
    output
}
