//! Remediation cases: TOOL/VER audit rows.
use velnor_actions_contract::ContractError;
use velnor_actions_contract::cachekey::{PlatformInputs, platform_id};
use velnor_actions_contract_config::config::OVERRIDABLE_ACTIONS;
use velnor_actions_contract_release::LATEST_RUNNER_LABEL;
use velnor_actions_contract_release::{
    FRESHNESS_CLASSES, Finding, FreshnessEntry, FreshnessRequirement, FreshnessStatus,
    GithubRunnerImages, NightlyRecord, PolicyException, RunnerImageEvidence, RunnerInventory,
    ToolIdentity, UNOBSERVED_IMAGE_VALUE, VersionPolicy, days_between, runner_family_changed,
    validate_freshness_class,
};

#[test]
fn tool_finding_carries_code_path_observed_action_reason() {
    let good = Finding {
        code: "missing_recommended_input".to_owned(),
        path: "mise.toml".to_owned(),
        observed: None,
        recommended: None,
        action: Some("add the recommended tools manually".to_owned()),
        reason: "Velnor cannot verify tool selection".to_owned(),
    };
    assert_eq!(good.validate(), Ok(()));
    let mut bad = good.clone();
    bad.code = "Missing-Code".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.path = "/abs/mise.toml".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.action = None;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.reason = "  ".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = good;
    bad.recommended = Some("2026.9.16".to_owned());
    assert_eq!(bad.validate(), Ok(()));
}

#[test]
fn ver_policy_header_rejects_weakening() {
    let good = sample_policy();
    assert_eq!(good.validate(".velnor/version-policy.toml"), Ok(()));
    let mut bad = good.clone();
    bad.channel = "beta".to_owned();
    assert!(bad.validate("p").is_err());
    let mut bad = good.clone();
    bad.max_exception_days = 30;
    let err = bad.validate("p").expect_err("weakening");
    assert!(err.to_string().contains("weakens_policy"));
    let mut bad = good.clone();
    bad.check_interval_hours = 72;
    assert!(bad.validate("p").is_err());
    let mut strict = good.clone();
    strict.max_exception_days = 7;
    strict.check_interval_hours = 12;
    assert_eq!(strict.validate("p"), Ok(()));
    let mut bad = good.clone();
    bad.registry = "not-a-url".to_owned();
    let err = bad.validate("p").expect_err("registry");
    assert!(err.to_string().contains("malformed_registry"));
    let mut bad = good.clone();
    bad.registry = "https://releases.example.com/rust".to_owned();
    assert_eq!(bad.validate("p"), Ok(()));
    let mut value = serde_json::to_value(&good).expect("value");
    value["weaken"] = serde_json::json!(true);
    assert!(serde_json::from_value::<VersionPolicy>(value).is_err());
}

/// Sample policy file content shared by VER cases.
fn sample_policy() -> VersionPolicy {
    VersionPolicy {
        schema: 1,
        channel: "stable".to_owned(),
        registry: "https://static.rust-lang.org/dist".to_owned(),
        check_interval_hours: 24,
        max_exception_days: 14,
        github_runner_images: GithubRunnerImages {
            linux_x64: RunnerInventory {
                default: LATEST_RUNNER_LABEL.to_owned(),
                supported: vec![
                    "ubuntu-26.04".to_owned(),
                    "ubuntu-24.04".to_owned(),
                    "ubuntu-22.04".to_owned(),
                ],
            },
        },
    }
}

#[test]
fn ver_runner_inventory_pins_latest_default() {
    assert_eq!(sample_policy().validate("p"), Ok(()));
    let mut policy = sample_policy();
    policy.github_runner_images.linux_x64.default = "ubuntu-24.04".to_owned();
    let err = policy.validate("p").expect_err("stale default");
    assert!(err.to_string().contains("must_equal_latest"));
    let mut policy = sample_policy();
    policy.github_runner_images.linux_x64.supported = vec!["ubuntu-latest".to_owned()];
    let err = policy.validate("p").expect_err("alias");
    assert!(err.to_string().contains("unsupported_label"));
    let mut policy = sample_policy();
    policy.github_runner_images.linux_x64.supported = vec!["ubuntu-24.04".to_owned()];
    assert!(policy.validate("p").is_err());
}

#[test]
fn ver_exception_window_and_expiry() -> Result<(), ContractError> {
    let hold = PolicyException {
        held_version: "1.2.3".to_owned(),
        owner: "team-ci".to_owned(),
        issue: "org/repo#42".to_owned(),
        reason: "upstream regression".to_owned(),
        granted: "2026-09-20".to_owned(),
        expires: "2026-10-04".to_owned(),
    };
    assert_eq!(hold.validate("p", 14), Ok(()));
    assert_eq!(days_between("2026-09-20", "2026-10-04"), Some(14));
    assert!(!hold.expired("2026-10-04")?);
    assert!(hold.expired("2026-10-05")?);
    let mut long = hold.clone();
    long.expires = "2026-10-05".to_owned();
    assert!(long.validate("p", 14).is_err());
    let mut empty = hold.clone();
    empty.owner = String::new();
    assert!(empty.validate("p", 14).is_err());
    let mut bad = hold;
    bad.expires = "2026-13-01".to_owned();
    assert!(bad.validate("p", 14).is_err());
    Ok(())
}

#[test]
fn ver_nightly_record_requires_dated_toolchain() -> Result<(), ContractError> {
    let record = NightlyRecord {
        toolchain: "nightly-2026-09-28".to_owned(),
        purpose: "miri".to_owned(),
        owner: "team-ci".to_owned(),
        qualified: "2026-09-28".to_owned(),
    };
    assert_eq!(record.validate("p"), Ok(()));
    assert!(record.qualification_current("2026-10-04")?);
    assert!(!record.qualification_current("2026-10-05")?);
    let mut moving = record.clone();
    moving.toolchain = "nightly".to_owned();
    let err = moving.validate("p").expect_err("moving nightly");
    assert!(err.to_string().contains("moving_nightly_forbidden"));
    let mut bad = record;
    bad.qualified = "yesterday".to_owned();
    assert!(bad.validate("p").is_err());
    Ok(())
}

#[test]
fn ver_freshness_entry_schema() {
    let entry = FreshnessEntry {
        component: "rust".to_owned(),
        current_pin: "1.98.1".to_owned(),
        latest_stable: "1.98.1".to_owned(),
        source_url: "https://releases.rs".to_owned(),
        checked_at: "2026-09-28T00:00:00Z".to_owned(),
        status: FreshnessStatus::Current,
        exception: None,
    };
    assert_eq!(entry.validate("inv"), Ok(()));
    for status in [
        FreshnessStatus::Current,
        FreshnessStatus::Stale,
        FreshnessStatus::Missing,
        FreshnessStatus::Mismatched,
        FreshnessStatus::Unreviewed,
        FreshnessStatus::ExpiredHold,
        FreshnessStatus::LookupFailed,
    ] {
        let mut entry = entry.clone();
        entry.status = status;
        assert_eq!(entry.validate("inv"), Ok(()), "{status:?}");
    }
    let mut bad = entry.clone();
    bad.checked_at = "soon".to_owned();
    assert!(bad.validate("inv").is_err());
    let mut bad = entry;
    bad.exception = Some(PolicyException {
        held_version: "1.0.0".to_owned(),
        owner: String::new(),
        issue: "x".to_owned(),
        reason: "y".to_owned(),
        granted: "2026-09-01".to_owned(),
        expires: "2026-09-10".to_owned(),
    });
    assert!(bad.validate("inv").is_err());
}

#[test]
fn ver_overridable_actions_are_exact_eight() {
    assert_eq!(OVERRIDABLE_ACTIONS.len(), 8);
    assert_eq!(
        OVERRIDABLE_ACTIONS,
        [
            "jdx/mise-action",
            "actions/checkout",
            "actions/download-artifact",
            "actions/upload-artifact",
            "actions/cache/restore",
            "actions/cache/save",
            "jdx/mr-boxington-action",
            "Swatinem/rust-cache",
        ]
    );
    // The Alint pin is policy-owned, not consumer-overridable
    // (docs/content/docs/proposed/version-policy.mdx §2 (GitHub Action defaults)).
    assert!(!OVERRIDABLE_ACTIONS.contains(&"asamarts/alint"));
}

#[test]
fn ver_platform_binds_runner_image() -> Result<(), ContractError> {
    let platform = |version: &str| {
        platform_id(&PlatformInputs {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            image_os: "ubuntu26".to_owned(),
            image_version: version.to_owned(),
            target: "host".to_owned(),
        })
    };
    let base = platform("20260928.1.0")?;
    assert_ne!(platform("20260929.1.0")?, base);
    assert_eq!(platform("20260928.1.0")?, base);
    Ok(())
}

#[test]
fn ver_tool_identity_carries_source_platforms_digest() {
    let tool = ToolIdentity {
        name: "rust".to_owned(),
        version: "1.98.1".to_owned(),
        source: "https://static.rust-lang.org/dist/channel-rust-stable.toml".to_owned(),
        platforms: vec!["linux-x64".to_owned()],
        digest: "ab".repeat(32),
    };
    assert_eq!(tool.validate("catalog"), Ok(()));
    let mut bad = tool.clone();
    bad.version = "1.98".to_owned();
    bad.platforms = vec![];
    assert!(bad.validate("catalog").is_err());
    let mut bad = tool.clone();
    bad.version = "stable".to_owned();
    assert!(bad.validate("catalog").is_err());
    let mut bad = tool.clone();
    bad.source = "https://releases.example.com/rust/latest".to_owned();
    assert!(bad.validate("catalog").is_err());
    let mut bad = tool.clone();
    bad.platforms = vec![];
    assert!(bad.validate("catalog").is_err());
    let mut bad = tool.clone();
    bad.digest = "xyz".to_owned();
    assert!(bad.validate("catalog").is_err());
    let mut bad = tool.clone();
    bad.digest = "00".repeat(32);
    let err = bad
        .validate("catalog")
        .expect_err("all-zero digests never validate as trusted");
    assert!(err.to_string().contains("placeholder_digest"), "{err}");
    let mut bad = tool;
    bad.name = "Rust!".to_owned();
    assert!(bad.validate("catalog").is_err());
}

#[test]
fn ver_freshness_classes_cover_nine_inputs() {
    assert_eq!(
        FRESHNESS_CLASSES,
        [
            "compiler",
            "bootstrap",
            "tools",
            "crates",
            "velnor",
            "actions",
            "alint",
            "runner",
            "deferred"
        ]
    );
    for class in FRESHNESS_CLASSES {
        assert_eq!(validate_freshness_class(class), Ok(()));
        let requirement = FreshnessRequirement {
            class: class.to_owned(),
            max_age_days: 30,
        };
        assert_eq!(requirement.validate("inv"), Ok(()), "{class}");
    }
    assert!(validate_freshness_class("packages").is_err());
    let zero = FreshnessRequirement {
        class: "crates".to_owned(),
        max_age_days: 0,
    };
    assert!(zero.validate("inv").is_err());
}

#[test]
fn ver_runner_image_evidence_and_family_change() {
    let evidence = RunnerImageEvidence {
        image_os: "ubuntu26".to_owned(),
        image_version: "20260928.1.0".to_owned(),
    };
    assert_eq!(evidence.validate(), Ok(()));
    let text = serde_json::to_string(&evidence).expect("serialize");
    assert!(text.contains("ubuntu26") && text.contains("20260928.1.0"));
    for overclaim in ["packages", "immutable", "fixed"] {
        assert!(!text.contains(overclaim), "overclaims {overclaim}");
    }
    let mut bad = evidence.clone();
    bad.image_version = String::new();
    assert!(bad.validate().is_err());
    let unobserved = RunnerImageEvidence::unobserved();
    assert!(unobserved.is_unobserved());
    assert_eq!(unobserved.validate(), Ok(()));
    assert_eq!(UNOBSERVED_IMAGE_VALUE, "unknown");
    assert!(!evidence.is_unobserved());
    let observed = RunnerImageEvidence::observed("ubuntu26", "20260928.1.0").expect("observed");
    assert!(!observed.is_unobserved());
    assert!(RunnerImageEvidence::observed("unknown", "20260928.1.0").is_err());
    assert!(RunnerImageEvidence::observed("ubuntu26", "unknown").is_err());
    assert!(!runner_family_changed("ubuntu-26.04", "ubuntu-26.04-arm"));
    assert!(runner_family_changed("ubuntu-24.04", "ubuntu-26.04"));
    assert!(runner_family_changed("ubuntu-26.04", "ubuntu-24.04-arm"));
}
