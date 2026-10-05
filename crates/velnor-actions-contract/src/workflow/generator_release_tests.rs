use super::{GeneratorReleasePlan, GeneratorReleaseTarget};
use crate::{RELEASE_MANIFEST_FILENAME, SUPPORTED_TARGETS};

#[test]
fn release_plan_binds_version_tag_targets_and_all_assets() {
    let plan = GeneratorReleasePlan::for_version("0.1.1").expect("release plan");
    assert_eq!(plan.version(), "0.1.1");
    assert_eq!(plan.tag(), "v0.1.1");
    assert_eq!(plan.repository(), "tailrocks/velnor-new");
    assert_eq!(
        plan.targets().map(GeneratorReleaseTarget::triple),
        SUPPORTED_TARGETS
    );
    assert_eq!(
        plan.targets().map(GeneratorReleaseTarget::runner_label),
        ["ubuntu-22.04", "macos-15"]
    );
    assert_eq!(
        plan.asset_names(),
        [
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
            "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
            "velnor-actions-0.1.1-aarch64-apple-darwin",
            "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
            RELEASE_MANIFEST_FILENAME,
        ]
    );
}

#[test]
fn release_plan_rejects_non_release_versions() {
    for version in ["", "0.1", "0.1.1-rc.1", "v0.1.1", "latest"] {
        assert!(GeneratorReleasePlan::for_version(version).is_err());
    }
}
