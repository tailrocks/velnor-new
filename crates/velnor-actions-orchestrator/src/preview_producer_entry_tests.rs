use super::*;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1,
    config::{RustBinaryPreviewArchive, RustBinaryPreviewProduct},
    package_release::PackageReleaseChannel,
};

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![RustBinaryPreviewProduct {
            id: "cli".to_owned(),
            package: "cli".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            archive: RustBinaryPreviewArchive::TarGz,
        }],
        companions: Vec::new(),
    }
}

#[test]
fn preview_entry_rejects_mismatched_workflow_authority() {
    let plan = plan();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let reference = "refs/heads/main";
    let workflow_ref = format!("OWNER/REPO/.github/workflows/binary-preview.yml@{reference}");
    assert!(workflow_identity_matches(
        &plan,
        "OWNER/REPO",
        reference,
        sha,
        sha,
        &workflow_ref
    ));
    for (repository, branch_ref, source_sha, authority_sha, workflow) in [
        ("attacker/repo", reference, sha, sha, workflow_ref.as_str()),
        (
            "owner/repo",
            "refs/heads/other",
            sha,
            sha,
            workflow_ref.as_str(),
        ),
        (
            "owner/repo",
            reference,
            sha,
            "ffffffffffffffffffffffffffffffffffffffff",
            workflow_ref.as_str(),
        ),
        (
            "owner/repo",
            reference,
            sha,
            sha,
            "owner/repo/.github/workflows/other.yml@refs/heads/main",
        ),
    ] {
        assert!(!workflow_identity_matches(
            &plan,
            repository,
            branch_ref,
            source_sha,
            authority_sha,
            workflow
        ));
    }
}
