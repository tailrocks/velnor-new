use super::{
    DistributionConfig, ExactHeadReviewCheck, HomebrewTapConfig, HomebrewTapDestination,
    HomebrewTapDestinations, ReviewAppIdentity, TapMergePolicy, TapUpdateEnvironment,
};
use crate::{
    homebrew_spec::{
        HOMEBREW_FORMULA_SPEC_SCHEMA_V1, HOMEBREW_FORMULA_TEMPLATE_V1, HomebrewFormulaChannelSpec,
        HomebrewFormulaSpec, HomebrewHostProductSpec, HomebrewStableSentinel,
    },
    package_release::PackageReleaseChannel,
};

fn formula_spec() -> HomebrewFormulaSpec {
    HomebrewFormulaSpec {
        schema: HOMEBREW_FORMULA_SPEC_SCHEMA_V1.to_owned(),
        template: HOMEBREW_FORMULA_TEMPLATE_V1.to_owned(),
        description: "Jackin command line tools".to_owned(),
        homepage: "https://github.com/jackin-project/jackin".to_owned(),
        license: "MIT".to_owned(),
        channels: vec![HomebrewFormulaChannelSpec {
            channel: PackageReleaseChannel::Preview,
            formula: "jackin-preview".to_owned(),
            formula_class: "JackinPreview".to_owned(),
            destination: "Formula/jackin-preview.rb".to_owned(),
            conflicts: vec!["jackin".to_owned()],
        }],
        host: HomebrewHostProductSpec {
            product: "jackin".to_owned(),
            targets: vec!["aarch64-apple-darwin".to_owned()],
            bins: vec!["jackin".to_owned()],
        },
        auxiliaries: Vec::new(),
        stable_sentinel: HomebrewStableSentinel {
            date: "2026-10-24".to_owned(),
            reason: "Stable release is not active".to_owned(),
        },
    }
}

pub(super) fn valid() -> HomebrewTapConfig {
    HomebrewTapConfig {
        enabled: true,
        destinations: HomebrewTapDestinations {
            preview: Some(HomebrewTapDestination {
                formula: "Formula/jackin-preview.rb".to_owned(),
            }),
            stable: None,
        },
        repository: "jackin-project/homebrew-tap".to_owned(),
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
fn rejects_unqualified_destination_or_credentials() {
    let mut config = valid();
    config.repository = "https://github.com/jackin-project/homebrew-tap".to_owned();
    assert!(config.validate("config.toml").is_err());
    config = valid();
    config
        .destinations
        .preview
        .as_mut()
        .expect("preview")
        .formula = "Formula/../evil.rb".to_owned();
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
fn closed_contract_rejects_arbitrary_environment_or_merge() {
    for (field, value) in [("environment", "production"), ("merge", "merge-if-green")] {
        let mut json = serde_json::to_value(valid()).expect("valid tap fixture");
        json[field] = serde_json::json!(value);
        assert!(
            serde_json::from_value::<HomebrewTapConfig>(json).is_err(),
            "{field}"
        );
    }
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

#[test]
fn signed_channels_select_distinct_declared_formula_authority() {
    use crate::package_release::PackageReleaseChannel;
    let mut config = valid();
    config.destinations.stable = Some(HomebrewTapDestination {
        formula: "Formula/jackin.rb".to_owned(),
    });
    assert!(config.validate("config.toml").is_ok());
    assert_eq!(
        config
            .destination(PackageReleaseChannel::Preview)
            .expect("preview")
            .formula,
        "Formula/jackin-preview.rb"
    );
    assert_eq!(
        config
            .destination(PackageReleaseChannel::Stable)
            .expect("stable")
            .formula,
        "Formula/jackin.rb"
    );
    config.destinations.stable.as_mut().expect("stable").formula =
        "Formula/jackin-preview.rb".to_owned();
    assert!(
        config
            .validate("config.toml")
            .expect_err("same destination")
            .to_string()
            .contains("channel_formula_paths_must_be_distinct")
    );
}

#[test]
fn tap_destinations_must_project_exactly_from_canonical_formula_spec() {
    let mut distribution = DistributionConfig {
        homebrew_tap: Some(valid()),
        homebrew_formula: Some(formula_spec()),
    };
    assert!(distribution.validate("config.toml").is_ok());
    distribution
        .homebrew_tap
        .as_mut()
        .expect("tap")
        .destinations
        .preview
        .as_mut()
        .expect("preview")
        .formula = "Formula/other.rb".to_owned();
    assert!(
        distribution
            .validate("config.toml")
            .expect_err("mismatch")
            .to_string()
            .contains("formula_destinations_must_match_homebrew_spec")
    );
}

#[test]
fn enabled_tap_requires_canonical_formula_spec_and_channel_set() {
    let mut distribution = DistributionConfig {
        homebrew_tap: Some(valid()),
        homebrew_formula: None,
    };
    assert!(
        distribution
            .validate("config.toml")
            .expect_err("missing spec")
            .to_string()
            .contains("enabled_homebrew_tap_requires_formula_spec")
    );
    distribution.homebrew_formula = Some(formula_spec());
    distribution
        .homebrew_tap
        .as_mut()
        .expect("tap")
        .destinations
        .stable = Some(HomebrewTapDestination {
        formula: "Formula/jackin.rb".to_owned(),
    });
    assert!(
        distribution
            .validate("config.toml")
            .expect_err("extra channel")
            .to_string()
            .contains("formula_destinations_must_match_homebrew_spec")
    );
}
