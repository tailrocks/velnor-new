use super::{
    DistributionConfig, ExactHeadReviewCheck, HomebrewTapConfig, HomebrewTapSource,
    ReviewAppIdentity, TapMergePolicy, TapUpdateEnvironment, TapUpdaterBinding,
};
use crate::config::MiseTaskSource;

pub(super) fn valid() -> HomebrewTapConfig {
    HomebrewTapConfig {
        enabled: true,
        source: HomebrewTapSource::RustBinaryReleasePreview,
        repository: "jackin-project/homebrew-tap".to_owned(),
        formula: "Formula/jackin-preview.rb".to_owned(),
        updater: TapUpdaterBinding {
            task: "package:update-preview".to_owned(),
            source: MiseTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: crate::config::CheckRunner {
                label: "ubuntu-26.04".to_owned(),
                platform: crate::config::CheckPlatform::LinuxX64,
                executor: crate::config::CheckExecutor::Hosted,
                container: None,
            },
            inputs: vec![
                "Formula/jackin-preview.rb".to_owned(),
                "mise.toml".to_owned(),
                "scripts/package-update.sh".to_owned(),
            ],
            tools: vec!["jq".to_owned(), "python".to_owned()],
            timeout_minutes: 5,
        },
        environment: TapUpdateEnvironment::TapUpdate,
        app_id_variable: "HOMEBREW_TAP_APP_ID".to_owned(),
        app_private_key_secret: "HOMEBREW_TAP_APP_PRIVATE_KEY".to_owned(),
        protection_observer: super::HomebrewTapProtectionObserver {
            app_id_variable: "TAP_PROTECTION_APP_ID".to_owned(),
            app_private_key_secret: "TAP_PROTECTION_APP_PRIVATE_KEY".to_owned(),
        },
        merge: TapMergePolicy::SquashAfterGates,
        review_completion: ExactHeadReviewCheck {
            check_name: "Review completion".to_owned(),
            app_id_variable: "REVIEW_APP_ID".to_owned(),
            expected_reviewers: vec![ReviewAppIdentity {
                app_id_variable: "BOT_A_APP_ID".to_owned(),
            }],
        },
    }
}

#[test]
fn accepts_native_tap_policy_and_defaults_distribution_empty() {
    assert_eq!(DistributionConfig::default().homebrew_tap, None);
    assert!(valid().validate("config.toml").is_ok());
}

#[test]
fn review_check_name_is_literal_policy_not_a_workflow_expression() {
    for name in [
        "${{ secrets.HOMEBREW_TAP_APP_PRIVATE_KEY }}",
        "Review ${{ github.sha }}",
        "Review\u{0085}complete",
    ] {
        let mut policy = valid();
        policy.review_completion.check_name = name.to_owned();
        assert!(policy.validate("config.toml").is_err(), "name {name:?}");
    }
    let mut policy = valid();
    policy.review_completion.check_name = "Révision terminée / exact head".to_owned();
    assert!(policy.validate("config.toml").is_ok());
}

#[test]
fn rejects_tap_runner_without_a_native_provisioning_lifecycle() {
    let mut config = valid();
    config.updater.runner.executor = crate::config::CheckExecutor::EphemeralSelfHosted;
    config.updater.runner.label = "tap-updater".to_owned();
    assert!(config.validate("config.toml").is_err());
}

#[test]
fn rejects_unqualified_destination_task_or_credentials() {
    let mut config = valid();
    config.repository = "https://github.com/jackin-project/homebrew-tap".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.formula = "Formula/../evil.rb".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.updater.task = "package:update-preview --token".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.app_id_variable = "GITHUB_TOKEN".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.app_private_key_secret = "APP_KEY".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.enabled = false;
    config.repository = "not-a-repository-url".to_owned();
    assert!(config.validate("config.toml").is_err());
}

#[test]
fn rejects_unqualified_inputs_tools_and_unbounded_task() {
    let mut config = valid();
    config.updater.inputs.push("../outside".to_owned());
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.updater.tools.push("jq".to_owned());
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.updater.timeout_minutes = 6;
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.updater.inputs.retain(|path| path != "mise.toml");
    assert!(config.validate("config.toml").is_err());
}

#[test]
fn review_completion_requires_named_check_and_trusted_app_id_reference() {
    let mut config = valid();
    config.review_completion.check_name = " Review completion".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.review_completion.app_id_variable = "HOMEBREW_TAP_APP_ID".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.review_completion.app_id_variable = "REVIEW_APP_PRIVATE_KEY".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config.review_completion.expected_reviewers.clear();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config
        .review_completion
        .expected_reviewers
        .push(ReviewAppIdentity {
            app_id_variable: "BOT_A_APP_ID".to_owned(),
        });
    assert!(config.validate("config.toml").is_err());
}

#[test]
fn closed_enums_reject_arbitrary_source_environment_and_merge_values() {
    for field in ["source", "environment", "merge"] {
        let value = match field {
            "source" => "manual-preview",
            "environment" => "production",
            _ => "merge-if-green",
        };
        let json = serde_json::to_string(&valid()).expect("serialize valid tap fixture");
        let json = json.replace(
            match field {
                "source" => "rust.binary_release.preview",
                "environment" => "tap-update",
                _ => "squash-after-gates",
            },
            value,
        );
        assert!(serde_json::from_str::<HomebrewTapConfig>(&json).is_err());
    }
}

#[test]
fn timeout_preserves_shorter_budgets_and_rejects_above_five() {
    for minutes in 1..=5 {
        let mut value = valid();
        value.updater.timeout_minutes = minutes;
        assert!(value.validate("config.toml").is_ok(), "{minutes}");
    }
    for minutes in [0, 6, 30, 360, u32::MAX] {
        let mut value = valid();
        value.updater.timeout_minutes = minutes;
        assert!(value.validate("config.toml").is_err(), "{minutes}");
    }
}

#[test]
fn omitted_timeout_uses_the_shared_five_minute_default() {
    let mut json = serde_json::to_value(valid().updater).expect("serialize");
    json.as_object_mut()
        .expect("object")
        .remove("timeout_minutes");
    let value: TapUpdaterBinding = serde_json::from_value(json).expect("default timeout");
    assert_eq!(value.timeout_minutes, 5);
}

#[test]
fn protection_observer_is_mandatory_closed_and_separate_from_writer() {
    let mut value = serde_json::to_value(valid()).expect("policy");
    value
        .as_object_mut()
        .expect("object")
        .remove("protection_observer");
    assert!(serde_json::from_value::<HomebrewTapConfig>(value).is_err());
    for (variable, secret) in [
        ("bad-variable", "KEY"),
        ("APP_ID", "bad-key"),
        ("HOMEBREW_TAP_APP_ID", "OBSERVER_KEY"),
        ("OBSERVER_ID", "HOMEBREW_TAP_APP_PRIVATE_KEY"),
    ] {
        let mut policy = valid();
        policy.protection_observer.app_id_variable = variable.to_owned();
        policy.protection_observer.app_private_key_secret = secret.to_owned();
        assert!(policy.validate("config.toml").is_err());
    }
    let mut value = serde_json::to_value(valid()).expect("policy");
    value["protection_observer"]["write"] = serde_json::json!(true);
    assert!(serde_json::from_value::<HomebrewTapConfig>(value).is_err());
}
