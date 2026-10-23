//! Characterize the original signed-recovery extractor before moving ZIP mechanics.
//! Payload byte format and bundle signatures are intentionally verified later.
use super::*;
use serde_json::json;
use std::{
    fs,
    io::{Cursor, Write},
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlanProduct,
    package_release::{
        PACKAGE_RELEASE_EVIDENCE_PROFILE, PACKAGE_RELEASE_SCHEMA_V2, PackageReleaseChannel,
        PackageReleasePayloadFormat,
    },
};

const PAYLOAD: &str = "app-aarch64-apple-darwin.tar.gz";
const BODY: &[u8] = b"asset-body";

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.into(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".into(),
        default_branch: "main".into(),
        base_version: "1.2.3".into(),
        products: vec![PackageReleasePlanProduct {
            id: "app".into(),
            package: "app".into(),
            bins: vec!["app".into()],
            targets: vec!["aarch64-apple-darwin".into()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
    }
}
fn receipt() -> PackageReleaseReceipt {
    let channel = PackageReleaseChannel::Preview;
    let mut assets = [
        (PAYLOAD.to_owned(), "payload"),
        (format!("{PAYLOAD}.sha256"), "sha256"),
        (format!("{PAYLOAD}.bundle"), "signature_bundle"),
        (format!("{PAYLOAD}.sbom.json"), "sbom"),
        ("SHA256SUMS".into(), "checksum_list"),
    ]
    .into_iter()
    .map(|(name, kind)| {
        json!({"name":name,"kind":kind,
        "product_id":if kind=="checksum_list" {None} else {Some("app")},
        "target":if kind=="checksum_list" {None} else {Some("aarch64-apple-darwin")},
        "subject_name":if kind=="checksum_list" {None} else {Some(PAYLOAD)},
        "size":BODY.len(),"sha256":sha256(BODY)})
    })
    .collect::<Vec<_>>();
    assets.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let receipt: PackageReleaseReceipt = serde_json::from_value(json!({
        "schema":PACKAGE_RELEASE_SCHEMA_V2,"channel":"preview", "repository":{"slug":"example/app","id":123},
        "source":{"commit_sha":"a".repeat(40),"ref":"refs/heads/main","default_branch":"main",
            "event":"workflow_run","ci_run_id":41,"ci_run_attempt":1},
        "workflow":{"path":channel.workflow_path(),"ref":"refs/heads/main","authority_sha":"a".repeat(40),
            "run_id":42,"run_attempt":2,"producer_run_id":42,"producer_run_attempt":2,
            "signer_job_id":channel.signer_job_id(),"signer_workflow":channel.workflow_path()},
        "version":{"base":"1.2.3","value":"1.2.3-preview.9+aaaaaaa","commit_count":9,"source_sha7":"aaaaaaa"},
        "tag":format!("preview-{}", "a".repeat(40)),
        "products":[{"id":"app","package":"app","package_version":"1.2.3","bins":["app"],
            "payloads":[{"format":"tar-gz","target":"aarch64-apple-darwin","name":PAYLOAD,"members":["app"]}]}],
        "companions":[],"assets":assets,
        "evidence":{"profile":PACKAGE_RELEASE_EVIDENCE_PROFILE,"sbom_format":"CycloneDX-JSON",
            "signer_issuer":"https://token.actions.githubusercontent.com",
            "signer_workflow_ref":"example/app/.github/workflows/binary-preview.yml@refs/heads/main",
            "attestation_predicate":"https://slsa.dev/provenance/v1","attested_subject_names":[PAYLOAD,"release.json"]}
    })).expect("typed receipt");
    receipt
        .validate_against_plan(&plan())
        .expect("complete valid signed inventory");
    receipt
}
fn entries(bundle: &[u8]) -> Vec<(String, Vec<u8>)> {
    let receipt = receipt();
    let mut entries = receipt
        .assets
        .iter()
        .map(|a| (a.name.clone(), BODY.to_vec()))
        .collect::<Vec<_>>();
    entries.push((
        "release.json".into(),
        receipt.canonical_bytes().expect("receipt"),
    ));
    entries.push(("release.json.bundle".into(), bundle.to_vec()));
    entries
}
fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644),
            )
            .expect("entry");
        writer.write_all(bytes).expect("entry bytes");
    }
    writer.finish().expect("closed archive").into_inner()
}
fn reject(bytes: &[u8]) {
    let output = tempfile::tempdir().expect("output");
    assert!(extract_signed_assets(bytes, output.path(), &plan()).is_err());
}

#[test]
fn original_valid_complete_receipt_and_unhashed_receipt_bundle_are_preserved() {
    for bundle in [
        b"first opaque signature bundle".as_slice(),
        b"different opaque bundle".as_slice(),
    ] {
        let output = tempfile::tempdir().expect("output");
        let actual = extract_signed_assets(&zip(&entries(bundle)), output.path(), &plan())
            .expect("valid original");
        assert_eq!(actual, receipt());
        assert_eq!(
            fs::read(output.path().join("release.json.bundle")).expect("bundle"),
            bundle
        );
        assert_eq!(
            fs::read(output.path().join(PAYLOAD)).expect("payload"),
            BODY
        );
        assert_eq!(
            fs::read_dir(output.path()).expect("inventory").count(),
            actual.assets.len() + 2
        );
    }
}

#[test]
fn original_path_unexpected_missing_and_digest_failures_reject() {
    let mut traversal = entries(b"bundle");
    traversal[0].0 = "../escaped".into();
    reject(&zip(&traversal));
    let mut unexpected = entries(b"bundle");
    unexpected[0].0 = "unlisted-file".into();
    reject(&zip(&unexpected));
    let mut missing = entries(b"bundle");
    missing.remove(0);
    reject(&zip(&missing));
    let mut digest = entries(b"bundle");
    digest[0].1 = b"wrong-body".to_vec();
    reject(&zip(&digest));
}

#[test]
fn original_nonregular_entry_is_rejected() {
    let mut bytes = zip(&entries(b"bundle"));
    let central = bytes
        .windows(4)
        .position(|b| b == b"PK\x01\x02")
        .expect("central");
    bytes[central + 38..central + 42].copy_from_slice(&(0o120_777_u32 << 16).to_le_bytes());
    reject(&bytes);
}

#[test]
fn repeated_receipt_hidden_by_zip_index_must_not_be_accepted() {
    let mut entries = entries(b"bundle");
    entries.push((
        "release.jsox".into(),
        receipt().canonical_bytes().expect("receipt"),
    ));
    let mut bytes = zip(&entries);
    for index in 0..=bytes.len() - 12 {
        if &bytes[index..index + 12] == b"release.jsox" {
            bytes[index + 11] = b'n';
        }
    }
    reject(&bytes);
}
