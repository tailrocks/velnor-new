use super::*;
use std::{io::Cursor, path::Path};

fn identity() -> Result<CosignCertificateIdentity, MiseError> {
    CosignCertificateIdentity::new(
        "jackin-project/velnor",
        ".github/workflows/binary-preview.yml",
        "refs/heads/main",
        "0123456789abcdef0123456789abcdef01234567",
        "workflow_run",
    )
}

#[test]
fn certificate_identity_is_exact_and_bound_to_workflow_ref() {
    assert_eq!(
        identity().map(|value| value.certificate_identity()),
        Ok("https://github.com/jackin-project/velnor/.github/workflows/binary-preview.yml@refs/heads/main".to_owned())
    );
}

#[test]
fn certificate_constraints_accept_workflow_run_and_reject_legacy_push() {
    assert_eq!(
        identity().expect("preview identity").trigger,
        "workflow_run"
    );
    assert!(
        CosignCertificateIdentity::new(
            "jackin-project/velnor",
            ".github/workflows/binary-preview.yml",
            "refs/heads/main",
            "0123456789abcdef0123456789abcdef01234567",
            "push"
        )
        .is_err()
    );
}

#[test]
fn certificate_identity_rejects_untrusted_identity_components() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let workflow = ".github/workflows/binary-preview.yml";
    let source_ref = "refs/heads/main";
    for (repository, workflow, source_ref, sha, trigger) in [
        (
            "jackin-project/velnor/other",
            workflow,
            source_ref,
            sha,
            "workflow_run",
        ),
        (
            "jackin-project/velnor",
            ".github/workflows/../evil.yml",
            "refs/heads/main",
            sha,
            "workflow_run",
        ),
        (
            "jackin-project/velnor",
            workflow,
            "refs/pull/4/merge",
            sha,
            "workflow_run",
        ),
        (
            "jackin-project/velnor",
            workflow,
            source_ref,
            "0123456789abcdef0123456789abcdef0123456G",
            "workflow_run",
        ),
        (
            "jackin-project/velnor",
            workflow,
            source_ref,
            sha,
            "pull_request",
        ),
    ] {
        assert!(
            CosignCertificateIdentity::new(repository, workflow, source_ref, sha, trigger).is_err()
        );
    }
}

#[test]
fn cosign_handle_rejects_relative_executable_paths() {
    assert!(PinnedCosign::from_verified_path(Path::new("cosign")).is_err());
}

#[test]
fn cosign_handle_rejects_unpinned_executable_bytes() {
    let mut bytes = Cursor::new(b"untrusted executable bytes");
    assert!(verify_cosign_reader(&mut bytes).is_err());
}

#[test]
fn cosign_measured_asset_length_reaches_digest_verification() {
    // Official v3.1.4 Linux amd64 asset is 145,688,404 bytes, exceeding the
    // former unrelated 128 MiB ceiling. Stream without allocating the asset.
    let mut input = std::io::repeat(0).take(145_688_404);
    let error = verify_cosign_reader_for(
        &mut input,
        cosign_artifact_for("linux", "x86_64").expect("pinned platform"),
    )
    .expect_err("synthetic bytes are not signed tool bytes");
    assert!(format!("{error:?}").contains("cosign_sha256_mismatch"));
    assert_eq!(
        input.limit(),
        0,
        "full measured asset can reach its digest guard"
    );
}

#[test]
fn cosign_measured_asset_ceiling_refuses_an_oversized_stream() {
    let mut input = std::io::repeat(0).take(145_688_405);
    let error = verify_cosign_reader_for(
        &mut input,
        cosign_artifact_for("linux", "x86_64").expect("pinned platform"),
    )
    .expect_err("oversized by one byte");
    assert!(format!("{error:?}").contains("cosign_file_too_large"));
    assert_eq!(
        input.limit(),
        0,
        "one excess byte proves the size violation"
    );
}

#[test]
fn verification_tool_profiles_are_closed_and_platform_specific() {
    let arm = cosign_artifact_for("macos", "aarch64").expect("reviewed ARM profile");
    let intel = cosign_artifact_for("macos", "x86_64").expect("reviewed Intel profile");
    assert_eq!(arm.name, "cosign-darwin-arm64");
    assert_eq!(arm.bytes, 99_023_586);
    assert_eq!(intel.name, "cosign-darwin-amd64");
    assert_ne!(arm.sha256, intel.sha256);
    assert!(cosign_artifact_for("windows", "aarch64").is_err());
    assert!(cosign_artifact_for("macos", "arm64").is_err());
}
