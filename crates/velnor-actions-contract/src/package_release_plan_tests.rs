use super::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    parse_package_release_plan,
};
use crate::package_release::{PackageReleaseChannel, PackageReleasePayloadFormat};

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "0.6.4".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["jackin".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
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
fn static_stable_plan_names_the_raw_single_binary_representation() {
    let mut plan = plan();
    plan.channel = crate::package_release::PackageReleaseChannel::Stable;
    plan.products[0].format = PackageReleasePayloadFormat::RawBinary;
    plan.products[0].targets = vec!["aarch64-apple-darwin".to_owned()];
    plan.validate().expect("valid stable raw payload plan");

    plan.products[0].format = PackageReleasePayloadFormat::TarGz;
    assert!(plan.validate().is_err(), "stable cannot claim a tarball");
}

#[test]
fn preview_plan_requires_main_while_stable_keeps_custom_default_branches() {
    let mut preview = plan();
    preview.default_branch = "develop".to_owned();
    assert!(preview.validate().is_err());

    let mut stable = plan();
    stable.channel = PackageReleaseChannel::Stable;
    stable.default_branch = "release/1.2".to_owned();
    stable.products[0].format = PackageReleasePayloadFormat::RawBinary;
    stable.products[0].targets = vec!["aarch64-apple-darwin".to_owned()];
    stable
        .validate()
        .expect("stable custom branch remains valid");
}

#[test]
fn static_plan_rejects_invalid_identity_and_unknown_executable_fields() {
    for (repository, branch, version) in [
        ("owner", "main", "0.6.4"),
        ("owner/repo/extra", "main", "0.6.4"),
        ("owner/repo", "../main", "0.6.4"),
        ("owner/repo", "release/.hidden", "0.6.4"),
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
