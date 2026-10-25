use super::assets::sha256;

const ADMISSION: &[u8] =
    include_bytes!("../../../.velnor/qualification/homebrew/preview-a-source-admission.json");
const ADMISSION_SHA256: &str = "73eacadf794d40c16e39e6693d09741ce9146cefc867d135f4a1fee023daa972";

#[test]
fn retained_admission_projects_with_pinned_identity_and_needs_no_artifact_api() {
    assert_eq!(ADMISSION.len(), 34_639);
    assert_eq!(sha256(ADMISSION), ADMISSION_SHA256);
    let projection = crate::release_policy_admission::project_public_historical_admission(
        ADMISSION,
        ADMISSION_SHA256,
    )
    .expect("original archived source admission is canonical and authenticated");
    assert_eq!(projection.repository_id, 1_197_700_841);
    assert_eq!(projection.head_repository_id, 1_197_700_841);
    assert_eq!(
        projection.source_sha,
        "41bad147b5ba9db09be99b07248531ce1bd867be"
    );
    assert_eq!(projection.source_ref, "refs/heads/main");
    assert_eq!(
        projection.workflow_authority_sha,
        "e737f4b5eb7c1a0d15ba6595fc260407ad178452"
    );
    assert_eq!(projection.run_id, 40_078_964_311);
    assert_eq!(projection.run_attempt, 2);
    assert_eq!(projection.ci_run_id, 40_036_544_370);
    assert_eq!(projection.ci_run_attempt, 1);
    assert_eq!(projection.commit_count, 1362);
    assert_eq!(projection.version, "0.6.4-preview.1362+41bad14");

    let recovery = include_str!("package_release_publish_public_recovery.rs");
    assert!(recovery.contains("project_public_historical_admission"));
    assert!(!recovery.contains("workflow_artifacts("));
    assert!(!recovery.contains("download_artifact_archive_bounded("));
}

#[test]
fn retained_admission_rejects_digest_and_canonical_byte_substitutions() {
    assert!(
        crate::release_policy_admission::project_public_historical_admission(
            ADMISSION,
            &"0".repeat(64),
        )
        .is_err()
    );

    let mut noncanonical = ADMISSION.to_vec();
    noncanonical.push(b' ');
    let digest = sha256(&noncanonical);
    assert!(
        crate::release_policy_admission::project_public_historical_admission(
            &noncanonical,
            &digest,
        )
        .is_err()
    );
}
