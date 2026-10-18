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
