//! Cross-policy admission and source-channel acceptance conditions.
use super::*;

fn config(stable: bool, preview: bool) -> VelnorConfig {
    let mut config: VelnorConfig = serde_json::from_value(serde_json::json!({
        "schema":1,
        "workflow":{"name":"CI","policy":"consumer-v1","generator_validation":"bootstrap","max_parallel_jobs":2},
        "resources":{"compiler_process_budget":2,"test_process_budget":2},
        "test_sharding":{"default_shards":1,"by_manifest":{}},
        "discovery":{"exclude":[]},
        "stacks":{"ignore":[],"rust":{"binary_release":{
            "enabled":stable,"package":"jackin","bin":"jackin",
            "preview":{
                "enabled":preview,"version_env":"JACKIN_VERSION_OVERRIDE",
                "evidence":{"profile":"signed-sbom-v1"},
                "producer":{"task":"release:preview-package","tools":["rust"],"inputs":["mise.toml","Cargo.lock"]},
                "products":[{"id":"jackin","package":"jackin","bins":["jackin"],"targets":["aarch64-apple-darwin"],"archive":"tar.gz"}]
            }
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
        .preview
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
                .contains("requires_enabled_binary_preview")
        );
        config.stacks.rust = None;
        assert!(
            config
                .validate("config.toml")
                .expect_err("missing source")
                .to_string()
                .contains("requires_enabled_binary_preview")
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
        .preview
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
