//! Cross-policy admission and source-channel acceptance conditions.
use super::*;

fn config(stable: bool, preview: bool) -> VelnorConfig {
    let mut config: VelnorConfig = serde_json::from_value(serde_json::json!({
        "schema":1,
        "workflow":{"name":"CI","policy":"consumer-v1","generator_validation":"bootstrap","max_parallel_jobs":2},
        "resources":{"compiler_process_budget":2,"test_process_budget":2},
        "test_sharding":{"default_shards":1,"by_manifest":{}},
        "discovery":{"exclude":[]},
        "stacks":{"ignore":[],"rust":{"release":{"enabled":stable,"registry":"git-tag","packages":["jackin"],"tag_name":"v{{ version }}"},"binary_release":{
            "version_package":"jackin", "version_env":"JACKIN_VERSION_OVERRIDE",
            "evidence":{"profile":"signed-sbom-v1"},
            "producer":{"task":"release:package","tools":["rust"],"inputs":["mise.toml","Cargo.lock"]},
            "products":[{"id":"jackin","package":"jackin","bins":["jackin"],"targets":["aarch64-apple-darwin"],"archive":"tar.gz"}],
            "stable":{"enabled":stable},
            "preview":{"enabled":preview,"release_id":328_385_904}
        }}}
    })).expect("typed config");
    config
        .qualified_tools
        .push(super::check_tool_reference_tests::declaration("rust"));
    config
}

#[test]
fn stable_and_preview_have_independent_enablement() {
    for stable in [false, true] {
        for preview in [false, true] {
            assert!(
                config(stable, preview).validate("config.toml").is_ok(),
                "stable={stable}, preview={preview}"
            );
        }
    }
}

#[test]
fn preview_only_cannot_escape_consumer_authorization() {
    let mut config = config(false, true);
    config.workflow.policy = WorkflowPolicy::VelnorRepositoryV1;
    assert!(
        config
            .validate("config.toml")
            .expect_err("policy")
            .to_string()
            .contains("binary_release_requires_consumer_policy")
    );
}

#[test]
fn preview_requires_explicit_qualified_tool_registry() {
    let mut config = config(false, true);
    config.qualified_tools.clear();
    assert!(
        config
            .validate("config.toml")
            .expect_err("unqualified")
            .to_string()
            .contains("unknown_qualified_tool:rust")
    );
}

#[test]
fn producer_tool_must_be_qualified_for_its_runner() {
    let mut config = config(false, true);
    config
        .stacks
        .rust
        .as_mut()
        .expect("rust")
        .binary_release
        .producer
        .runner = VerificationRunner::MacosArm64;
    assert!(
        config
            .validate("config.toml")
            .expect_err("platform mismatch")
            .to_string()
            .contains("qualified_tool_missing_producer_platform:rust")
    );
}

#[test]
fn distribution_requires_enabled_preview_even_when_stable_enabled() {
    for stable in [false, true] {
        let mut config = config(stable, false);
        config.distribution.homebrew_tap = Some(super::distribution::valid_tap());
        assert!(
            config
                .validate("config.toml")
                .expect_err("disabled source")
                .to_string()
                .contains("requires_enabled_binary_channel")
        );
        config.stacks.rust = None;
        assert!(
            config
                .validate("config.toml")
                .expect_err("missing source")
                .to_string()
                .contains("requires_enabled_binary_channel")
        );
    }
}

#[test]
fn distribution_admits_enabled_preview_and_can_be_independently_disabled() {
    let mut config = config(false, true);
    config.distribution.homebrew_tap = Some(super::distribution::valid_tap());
    assert!(config.validate("config.toml").is_ok());
    config
        .distribution
        .homebrew_tap
        .as_mut()
        .expect("tap")
        .enabled = false;
    config.stacks.rust = None;
    assert!(config.validate("config.toml").is_ok());
}

#[test]
fn producer_respects_repository_compiler_budget() {
    let mut config = config(false, true);
    config
        .stacks
        .rust
        .as_mut()
        .expect("rust")
        .binary_release
        .producer
        .build_jobs = 3;
    assert!(
        config
            .validate("config.toml")
            .expect_err("budget")
            .to_string()
            .contains("exceeds_compiler_process_budget")
    );
}

#[test]
fn native_binary_channels_require_the_canonical_ci_name() {
    for (stable, preview) in [(true, false), (false, true), (true, true)] {
        for name in ["Custom CI", "ci", " CI", "CI "] {
            let mut value = config(stable, preview);
            value.workflow.name = name.to_owned();
            let error = value
                .validate("config.toml")
                .expect_err("noncanonical native CI");
            assert!(
                matches!(error, ContractError::Config { ref key_path, ref problem, .. }
                if key_path == "workflow.name" && problem == "native_binary_release_requires_canonical_ci_name:CI"),
                "{error}"
            );
        }
        assert!(config(stable, preview).validate("config.toml").is_ok());
    }
}

#[test]
fn custom_ci_names_remain_valid_without_native_binary_publication() {
    for name in ["Custom CI", "ci", " CI", "CI "] {
        let mut value = config(false, false);
        value.workflow.name = name.to_owned();
        assert!(value.validate("config.toml").is_ok(), "{name:?}");
        value.stacks.rust = None;
        assert!(value.validate("config.toml").is_ok(), "{name:?}");
    }
}

#[test]
fn native_consumer_authorization_error_precedes_ci_name_restriction() {
    let mut value = config(false, true);
    value.workflow.name = "Custom CI".to_owned();
    value.workflow.policy = WorkflowPolicy::VelnorRepositoryV1;
    let error = value
        .validate("config.toml")
        .expect_err("nonconsumer native policy");
    assert!(
        matches!(error, ContractError::Config { ref key_path, ref problem, .. }
        if key_path == "stacks.rust.binary_release" && problem == "binary_release_requires_consumer_policy"),
        "{error}"
    );
}

#[test]
fn generic_ci_name_validation_retains_its_original_diagnostics() {
    for (name, expected) in [
        ("", "empty_name"),
        ("${{ github.workflow }}", "bad_name"),
        ("CI\n", "bad_name"),
    ] {
        let mut value = config(false, false);
        value.workflow.name = name.to_owned();
        let error = value
            .validate("config.toml")
            .expect_err("invalid generic name");
        assert!(
            matches!(error, ContractError::Config { ref key_path, ref problem, .. }
            if key_path == "workflow.name" && problem == expected),
            "{error}"
        );
    }
}

#[test]
fn native_stable_requires_the_matching_manual_git_tag_version_owner() {
    let baseline = config(true, false);
    assert!(baseline.validate("config.toml").is_ok());
    for mutation in 0..6 {
        let mut value = baseline.clone();
        let rust = value.stacks.rust.as_mut().expect("rust policy");
        match mutation {
            0 => rust.release.enabled = false,
            1 => rust.release.registry = ReleaseRegistry::CratesIo,
            2 => rust.release.packages = vec!["other".to_owned()],
            3 => rust.release.manifest_path = "other/Cargo.toml".to_owned(),
            4 => rust.release.environment = "other".to_owned(),
            _ => rust.release.tag_name = "{{ package }}-v{{ version }}".to_owned(),
        }
        assert!(
            value
                .validate("config.toml")
                .expect_err("mismatched stable owner")
                .to_string()
                .contains("requires_matching_manual_git_tag_release")
        );
    }
}

#[test]
fn stable_destination_requires_stable_and_never_borrows_preview_authority() {
    let mut value = config(false, true);
    let mut tap = super::distribution::valid_tap();
    tap.destinations.preview = None;
    tap.destinations.stable = Some(HomebrewTapDestination {
        formula: "Formula/jackin.rb".to_owned(),
    });
    tap.updater.inputs.push("Formula/jackin.rb".to_owned());
    value.distribution.homebrew_tap = Some(tap);
    assert!(
        value
            .validate("config.toml")
            .expect_err("stable disabled")
            .to_string()
            .contains("requires_enabled_binary_channel")
    );
    let rust = value.stacks.rust.as_mut().expect("rust");
    rust.binary_release.stable.enabled = true;
    rust.release.enabled = true;
    assert!(value.validate("config.toml").is_ok());
}
