//! Service digest and manifest identity stay separate at acquisition.

use super::*;

fn receipt(bytes: &[u8], sha256: &str) -> BaselineArtifactReceipt {
    BaselineArtifactReceipt {
        service_id: 701,
        sha256: sha256.to_owned(),
        size_bytes: u64::try_from(bytes.len()).expect("byte count"),
        name: "velnor-baseline-example".to_owned(),
        compatibility_id: "b3-test".to_owned(),
        run_id: 7,
        head_sha: "1".repeat(40),
        head_branch: "main".to_owned(),
    }
}

#[test]
fn archive_download_uses_the_service_id_endpoint() {
    let lookup = crate::cover::shard_baseline::BaselineLookup::new(
        &"1".repeat(40),
        ".github/workflows/ci.yml",
        "main",
        "o/r",
    )
    .expect("lookup");
    let service = receipt(b"bytes", &"a".repeat(64));
    let args = artifact_archive_args(&lookup, &service)
        .expect("service archive endpoint")
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "api".to_owned(),
            "repos/o/r/actions/artifacts/701/zip".to_owned(),
        ]
    );
}

#[test]
fn service_digest_is_checked_before_zip_parser() {
    let bytes = b"not a ZIP archive";
    let error = verify_archive(receipt(bytes, &"0".repeat(64)), bytes).err();
    assert_eq!(error.as_deref(), Some("baseline_artifact_digest_mismatch"));
}

#[test]
fn byte_count_is_checked_before_digest_or_zip_parser() {
    let bytes = b"not a ZIP archive";
    let mut service = receipt(bytes, &sha256(bytes));
    service.size_bytes += 1;
    let error = verify_archive(service, bytes).err();
    assert_eq!(error.as_deref(), Some("baseline_archive_unavailable"));
}

#[test]
fn service_id_is_not_the_manifest_name_fingerprint() {
    let base = "1".repeat(40);
    let mut manifest = crate::cover::revalidate::cover_revalidate_fixtures::manifest_for(&base);
    manifest.run_attempt = 2;
    manifest.artifact_name =
        velnor_actions_contract::artifact_id_for_baseline(&base, &manifest.compatibility_id)
            .expect("source/compatibility name");
    manifest.artifact_id =
        crate::cover_compat::baseline_artifact_numeric_id(&manifest.artifact_name);
    let service = BaselineArtifactReceipt {
        service_id: 999,
        sha256: "a".repeat(64),
        size_bytes: 100,
        name: manifest.artifact_name.clone(),
        compatibility_id: manifest.compatibility_id.clone(),
        run_id: 7,
        head_sha: base,
        head_branch: "testmain".to_owned(),
    };
    assert_ne!(manifest.artifact_id, service.service_id);
    assert!(manifest_matches_receipt(&manifest, &service));
    assert!(
        service.name.ends_with(&service.compatibility_id),
        "attempt stays in the manifest/API proof, never the artifact name"
    );
}
