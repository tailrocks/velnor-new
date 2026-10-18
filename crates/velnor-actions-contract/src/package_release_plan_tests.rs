use super::{PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, parse_package_release_plan};
use crate::{config::RustBinaryPreviewProduct, package_release::PackageReleaseChannel};

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "0.6.4".to_owned(),
        products: vec![RustBinaryPreviewProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["jackin".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            archive: crate::config::RustBinaryPreviewArchive::TarGz,
        }],
        companions: Vec::new(),
    }
}

#[test]
fn static_plan_round_trips_canonically_without_task_payload() {
    let plan = plan();
    let bytes = plan.canonical_bytes().expect("canonical bytes");
    assert_eq!(parse_package_release_plan(&bytes).expect("parse"), plan);
    let text = String::from_utf8(bytes).expect("UTF-8");
    assert!(!text.contains("run"));
    assert!(text.contains("package-release-plan.v1"));
}

#[test]
fn static_plan_rejects_invalid_identity_and_unknown_executable_fields() {
    for (repository, branch, version) in [
        ("owner", "main", "0.6.4"),
        ("owner/repo/extra", "main", "0.6.4"),
        ("owner/repo", "../main", "0.6.4"),
        ("owner/repo", "main", "0.6.4-preview.1"),
    ] {
        let mut candidate = plan();
        candidate.repository = repository.to_owned();
        candidate.default_branch = branch.to_owned();
        candidate.base_version = version.to_owned();
        assert!(candidate.validate().is_err());
    }
    let mut bytes = plan().canonical_bytes().expect("bytes");
    let text = String::from_utf8(bytes.clone()).expect("UTF-8");
    let injected = text.replace("\"schema\":", "\"task\":\"evil\",\"schema\":");
    bytes = injected.into_bytes();
    assert!(parse_package_release_plan(&bytes).is_err());
}
