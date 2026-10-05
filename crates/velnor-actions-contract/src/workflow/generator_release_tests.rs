use super::{GeneratorReleasePlan, GeneratorReleaseSourceBinding, GeneratorReleaseTarget};
use crate::{RELEASE_MANIFEST_FILENAME, SUPPORTED_TARGETS};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn release_plan_binds_version_source_tag_targets_and_metadata() {
    let plan =
        GeneratorReleasePlan::for_version_and_source("0.1.1", SOURCE_SHA).expect("release plan");
    assert_eq!(plan.version(), "0.1.1");
    assert_eq!(plan.source_sha(), SOURCE_SHA);
    assert_eq!(plan.tag(), format!("generator-{SOURCE_SHA}"));
    assert_eq!(plan.repository(), "tailrocks/velnor-new");
    assert_eq!(
        plan.targets().map(GeneratorReleaseTarget::triple),
        SUPPORTED_TARGETS
    );
    assert_eq!(
        plan.targets().map(GeneratorReleaseTarget::runner_label),
        ["ubuntu-22.04", "macos-15"]
    );
}

#[test]
fn release_plan_exposes_staged_and_final_asset_inventories() {
    let plan =
        GeneratorReleasePlan::for_version_and_source("0.1.1", SOURCE_SHA).expect("release plan");
    assert_eq!(
        plan.staged_asset_names(),
        [
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
            "velnor-actions-0.1.1-aarch64-apple-darwin",
            "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
        ]
    );
    assert_eq!(
        plan.final_asset_names(),
        [
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
            "velnor-actions-0.1.1-aarch64-apple-darwin",
            "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
            RELEASE_MANIFEST_FILENAME,
            "velnor-actions-release-manifest.json.sha256",
        ]
    );
}

#[test]
fn release_plan_rejects_non_semver_release_versions() {
    for version in [
        "",
        "0.1",
        "0.1.1-rc.1",
        "v0.1.1",
        "latest",
        "01.1.1",
        "1.01.1",
        "1.1.01",
        "18446744073709551616.1.0",
    ] {
        assert!(
            GeneratorReleasePlan::for_version_and_source(version, SOURCE_SHA).is_err(),
            "accepted invalid release version {version:?}"
        );
    }
}

#[test]
fn release_plan_rejects_invalid_source_sha_values() {
    let invalid_source_shas = [
        String::new(),
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        format!("{}g", "a".repeat(39)),
    ];
    for source_sha in invalid_source_shas {
        assert!(
            GeneratorReleasePlan::for_version_and_source("0.1.1", &source_sha).is_err(),
            "accepted invalid source SHA {source_sha:?}"
        );
    }
}

#[test]
fn current_workflow_binding_uses_only_the_fixed_github_sha_expression() {
    let binding =
        GeneratorReleaseSourceBinding::for_current_workflow("0.1.1").expect("source binding");
    assert_eq!(binding.source_expression(), "${{ github.sha }}");

    for expression in [
        "${{ github.ref }}",
        "${{ env.GITHUB_SHA }}",
        "${{ github.sha }}-suffix",
        "$GITHUB_SHA",
    ] {
        assert!(
            GeneratorReleasePlan::for_version_and_source("0.1.1", expression).is_err(),
            "accepted unbound expression as an exact source SHA: {expression:?}"
        );
    }
}

#[test]
fn current_workflow_binding_resolves_to_an_exact_source_bound_plan() {
    let binding =
        GeneratorReleaseSourceBinding::for_current_workflow("0.1.1").expect("source binding");
    assert_eq!(binding.version(), "0.1.1");
    assert_eq!(
        binding.targets().map(GeneratorReleaseTarget::triple),
        SUPPORTED_TARGETS
    );
    assert_eq!(
        binding.staged_asset_names(),
        [
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
            "velnor-actions-0.1.1-aarch64-apple-darwin",
            "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
        ]
    );
    let plan = binding.bind(SOURCE_SHA).expect("bound release plan");

    assert_eq!(plan.source_sha(), SOURCE_SHA);
    assert_eq!(plan.tag(), format!("generator-{SOURCE_SHA}"));
    assert_eq!(
        plan.final_asset_names(),
        [
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
            "velnor-actions-0.1.1-aarch64-apple-darwin",
            "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
            RELEASE_MANIFEST_FILENAME,
            "velnor-actions-release-manifest.json.sha256",
        ]
    );
}

#[test]
fn current_workflow_binding_rejects_invalid_runtime_source_sha() {
    let binding =
        GeneratorReleaseSourceBinding::for_current_workflow("0.1.1").expect("source binding");
    assert!(binding.bind("${{ github.sha }}").is_err());
    assert!(binding.bind(&"A".repeat(40)).is_err());
}
