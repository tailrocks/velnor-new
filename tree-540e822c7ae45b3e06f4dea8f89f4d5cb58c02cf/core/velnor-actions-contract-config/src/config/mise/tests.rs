//! Explicit check schema regression cases.
use super::*;

fn check() -> MiseCheck {
    MiseCheck {
        id: "ffi".to_owned(),
        task: "test:ffi".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "macos-15".to_owned(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["mise.toml".to_owned()],
        tools: vec!["swift".to_owned()],
        system_tools: Vec::new(),
        evidence: Some(CheckEvidence {
            path: "evidence/ffi.json".to_owned(),
            expected_scenarios: vec!["native".to_owned()],
        }),
        timeout_minutes: 30,
    }
}

#[test]
fn check_names_paths_inputs_and_scenarios_fail_closed() {
    assert!(check().validate("config.toml", "checks[0]").is_ok());
    for value in ["", "-flag", "a b", "${{ env.X }}", "$HOME", "a;true", "a/b"] {
        let mut invalid = check();
        invalid.task = value.to_owned();
        assert!(
            invalid.validate("config.toml", "checks[0]").is_err(),
            "{value:?}"
        );
    }
    for value in ["", "../outside", "/absolute", "a/../b", "$HOME", "a\\b"] {
        let mut invalid = check();
        invalid.directory = value.to_owned();
        assert!(
            invalid.validate("config.toml", "checks[0]").is_err(),
            "{value:?}"
        );
    }
    let mut invalid = check();
    invalid.inputs.clear();
    assert!(invalid.validate("config.toml", "checks[0]").is_err());
    let mut invalid = check();
    invalid
        .evidence
        .as_mut()
        .expect("evidence")
        .expected_scenarios
        .clear();
    assert!(invalid.validate("config.toml", "checks[0]").is_err());
}

#[test]
fn platform_and_executor_are_independent() {
    let mut runner = check().runner;
    runner.platform = CheckPlatform::MacosX64;
    assert!(runner.validate("config.toml", "runner").is_err());
    runner.label = "macos-15-intel".to_owned();
    assert!(runner.validate("config.toml", "runner").is_ok());
    runner.executor = CheckExecutor::EphemeralSelfHosted;
    assert!(runner.validate("config.toml", "runner").is_err());
    runner.label = "native-ffi-scale-set".to_owned();
    assert!(runner.validate("config.toml", "runner").is_ok());
    for label in ["self-hosted", "latest", "macos-latest", "${{ env.RUNNER }}"] {
        runner.label = label.to_owned();
        assert!(runner.validate("config.toml", "runner").is_err());
    }
}

#[test]
fn check_platform_maps_to_canonical_release_target() {
    for (platform, target) in [
        (
            CheckPlatform::LinuxX64,
            velnor_actions_contract_release::ReleaseTarget::LinuxX86_64,
        ),
        (
            CheckPlatform::MacosArm64,
            velnor_actions_contract_release::ReleaseTarget::MacosArm64,
        ),
        (
            CheckPlatform::MacosX64,
            velnor_actions_contract_release::ReleaseTarget::MacosX86_64,
        ),
    ] {
        assert_eq!(platform.release_target(), target);
        assert_eq!(platform.target(), target.triple());
    }
}

#[test]
fn unknown_execution_fields_and_executor_are_rejected() {
    for value in [
        r#"{"label":"native","platform":"macos_arm64","executor":"hosted","capabilities":[]}"#,
        r#"{"label":"native","platform":"macos_arm64","executor":"hosted","docker_context":"default"}"#,
        r#"{"label":"native","platform":"macos_arm64","executor":"self_hosted"}"#,
        r#"{"label":"native","platform":"macos_arm64","executor":"hosted","env":{"X":"Y"}}"#,
    ] {
        assert!(serde_json::from_str::<CheckRunner>(value).is_err());
    }
}
#[test]
fn arbitrary_execution_configuration_is_rejected() {
    for key in ["env", "argv", "commands", "plugin_url", "yaml"] {
        let mut value = serde_json::to_value(check()).expect("check serialization");
        value
            .as_object_mut()
            .expect("object")
            .insert(key.to_owned(), serde_json::json!("evil"));
        assert!(serde_json::from_value::<MiseCheck>(value).is_err(), "{key}");
    }
    let mut invalid = check();
    invalid
        .evidence
        .as_mut()
        .expect("evidence")
        .expected_scenarios = vec!["z".to_owned(), "a".to_owned()];
    assert!(invalid.validate("config.toml", "checks[0]").is_err());
    let mut hidden = check();
    hidden.inputs = vec![".github/workflows/ci.yml".to_owned()];
    assert!(hidden.validate("config.toml", "checks[0]").is_ok());
}
#[test]
fn native_only_check_requires_no_mise_installed_tools() {
    let mut native = check();
    native.tools.clear();
    native.system_tools = vec![CheckSystemTool {
        kind: CheckSystemToolKind::Swift,
        version: "6.2.1".to_owned(),
        build: "swiftlang-6.2.1.1.1".to_owned(),
    }];
    assert!(native.validate("config.toml", "checks[0]").is_ok());
    native.system_tools[0].build = "$(untrusted)".to_owned();
    assert!(native.validate("config.toml", "checks[0]").is_err());
}
