//! Strict observed platform identities remain separate from planned unknowns.

use velnor_actions_contract::cachekey::{
    MAX_OBSERVED_PLATFORM_FACT_BYTES, PlatformInputs, observed_platform_id, platform_id,
};
use velnor_actions_contract::{
    ContractError, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment,
    PlatformUnavailableReason, RunnerImageEvidence, UNOBSERVED_IMAGE_VALUE,
};

fn inputs() -> PlatformInputs {
    PlatformInputs {
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        image_os: "ubuntu26".to_owned(),
        image_version: "20261004.1.0".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
    }
}

#[test]
fn observed_identity_uses_the_canonical_platform_preimage() {
    let facts = inputs();
    assert_eq!(
        observed_platform_id(&facts).expect("observed ID"),
        platform_id(&facts).expect("canonical ID")
    );
    let mut changed = facts;
    changed.image_version = "20261005.1.0".to_owned();
    assert_ne!(
        observed_platform_id(&changed).expect("changed observed ID"),
        observed_platform_id(&inputs()).expect("original observed ID")
    );
}

#[test]
fn observed_identity_rejects_missing_unknown_and_oversized_facts() {
    let mut missing = inputs();
    missing.image_os.clear();
    assert!(observed_platform_id(&missing).is_err());

    for field in ["os", "arch", "runs_on", "target"] {
        let mut unknown = inputs();
        match field {
            "os" => unknown.os = UNOBSERVED_IMAGE_VALUE.to_owned(),
            "arch" => unknown.arch = UNOBSERVED_IMAGE_VALUE.to_owned(),
            "runs_on" => unknown.runs_on = UNOBSERVED_IMAGE_VALUE.to_owned(),
            "target" => unknown.target = UNOBSERVED_IMAGE_VALUE.to_owned(),
            _ => unreachable!(),
        }
        assert!(observed_platform_id(&unknown).is_err(), "{field}");
    }
    assert!(RunnerImageEvidence::observed("unknown", "20261004.1.0").is_err());

    let mut oversized = inputs();
    oversized.image_version = "x".repeat(MAX_OBSERVED_PLATFORM_FACT_BYTES + 1);
    assert_eq!(
        observed_platform_id(&oversized).expect_err("oversized fact"),
        ContractError::identity("image_version", "fact_too_long")
    );
}

#[test]
fn report_binding_proves_the_observation_matches_the_planned_platform() {
    let observed = inputs();
    let planned =
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform");
    let planned_id = planned.platform_id.clone();
    let binding = PlatformBinding::observed(
        &planned_id,
        PlatformRunnerEnvironment::GithubHosted,
        observed.clone(),
    )
    .expect("binding");
    binding
        .validate_for_plan(&planned)
        .expect("valid plan binding");
    let custom = PlannedPlatform::new(
        "scale-set:velnor+ubuntu-26.04-scale-set",
        "x86_64-unknown-linux-gnu",
    )
    .expect("custom runner platform");
    assert!(binding.validate_for_plan(&custom).is_err());

    let other_label = PlannedPlatform::new("ubuntu-24.04", "x86_64-unknown-linux-gnu")
        .expect("other hosted platform");
    assert!(binding.validate_for_plan(&other_label).is_err());
    let other_target = PlannedPlatform::new("ubuntu-26.04", "aarch64-apple-darwin")
        .expect("other target platform");
    assert!(binding.validate_for_plan(&other_target).is_err());
    let other_identity = PlannedPlatform::new("ubuntu-24.04", "x86_64-unknown-linux-gnu")
        .expect("other planned identity");
    assert!(binding.validate_for_plan(&other_identity).is_err());

    let unavailable = PlatformBinding::unavailable(
        &custom.platform_id,
        PlatformRunnerEnvironment::SelfHosted,
        PlatformUnavailableReason::CustomRunner,
    )
    .expect("unavailable binding");
    unavailable
        .validate_for_plan(&custom)
        .expect("unavailable evidence still binds the plan");
}

#[test]
fn only_github_hosted_observations_can_be_admitted() {
    let mut planned = inputs();
    planned.image_os = UNOBSERVED_IMAGE_VALUE.to_owned();
    planned.image_version = UNOBSERVED_IMAGE_VALUE.to_owned();
    let planned_id = platform_id(&planned).expect("planned ID");

    assert!(
        PlatformBinding::observed(&planned_id, PlatformRunnerEnvironment::SelfHosted, inputs(),)
            .is_err()
    );
    assert!(
        PlatformBinding::unavailable(
            &planned_id,
            PlatformRunnerEnvironment::GithubHosted,
            PlatformUnavailableReason::CustomRunner,
        )
        .is_err()
    );
    assert_eq!(
        PlatformRunnerEnvironment::parse("github-hosted"),
        PlatformRunnerEnvironment::GithubHosted
    );
    assert_eq!(
        PlatformRunnerEnvironment::parse("self-hosted"),
        PlatformRunnerEnvironment::SelfHosted
    );
    assert_eq!(
        PlatformRunnerEnvironment::parse("unexpected"),
        PlatformRunnerEnvironment::Unknown
    );
}
