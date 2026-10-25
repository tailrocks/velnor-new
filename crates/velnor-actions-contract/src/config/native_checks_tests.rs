use super::*;

fn check(operation: NativeHomebrewOperation) -> NativeHomebrewCheck {
    NativeHomebrewCheck {
        id: "preview-install".to_owned(),
        operation,
        formula_channel: match operation {
            NativeHomebrewOperation::FormulaContract | NativeHomebrewOperation::Install => {
                Some(PackageReleaseChannel::Preview)
            }
            NativeHomebrewOperation::TapTree | NativeHomebrewOperation::AdoptionContract => None,
        },
        runner: CheckRunner {
            label: "macos-15".to_owned(),
            platform: crate::config::CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        timeout_minutes: if operation == NativeHomebrewOperation::Install {
            15
        } else {
            5
        },
    }
}

#[test]
fn native_check_operation_channel_and_hosted_runner_are_closed() {
    for operation in [
        NativeHomebrewOperation::TapTree,
        NativeHomebrewOperation::FormulaContract,
        NativeHomebrewOperation::Install,
        NativeHomebrewOperation::AdoptionContract,
    ] {
        let result = check(operation).validate("config.toml", "native_checks[0]");
        assert!(result.is_ok(), "{operation:?}: {result:?}");
    }

    let mut invalid = check(NativeHomebrewOperation::TapTree);
    invalid.formula_channel = Some(PackageReleaseChannel::Preview);
    assert!(invalid.validate("config.toml", "native_checks[0]").is_err());
    invalid = check(NativeHomebrewOperation::Install);
    invalid.formula_channel = None;
    assert!(invalid.validate("config.toml", "native_checks[0]").is_err());
    invalid = check(NativeHomebrewOperation::TapTree);
    invalid.runner.executor = CheckExecutor::EphemeralSelfHosted;
    invalid.runner.label = "preview-runner".to_owned();
    assert!(invalid.validate("config.toml", "native_checks[0]").is_err());
}

#[test]
fn native_check_timeouts_follow_closed_operation_budgets() {
    let mut declaration = check(NativeHomebrewOperation::TapTree);
    declaration.timeout_minutes = 6;
    assert!(
        declaration
            .validate("config.toml", "native_checks[0]")
            .is_err()
    );
    declaration = check(NativeHomebrewOperation::Install);
    declaration.timeout_minutes = 31;
    assert!(
        declaration
            .validate("config.toml", "native_checks[0]")
            .is_err()
    );
}

#[test]
fn native_install_requires_hosted_macos_arm_but_other_operations_keep_platform_support() {
    use crate::config::CheckPlatform;

    let unsupported = [
        (CheckPlatform::LinuxX64, "ubuntu-26.04"),
        (CheckPlatform::MacosX64, "macos-15-intel"),
    ];
    for (platform, label) in unsupported {
        let mut install = check(NativeHomebrewOperation::Install);
        install.runner.platform = platform;
        install.runner.label = label.to_owned();
        let error = install
            .validate("config.toml", "native_checks[0]")
            .expect_err("native Homebrew install is ARM macOS only");
        assert!(
            error
                .to_string()
                .contains("native_homebrew_install_requires_macos_arm64"),
            "{platform:?}: {error}"
        );

        for operation in [
            NativeHomebrewOperation::TapTree,
            NativeHomebrewOperation::FormulaContract,
            NativeHomebrewOperation::AdoptionContract,
        ] {
            let mut declaration = check(operation);
            declaration.runner.platform = platform;
            declaration.runner.label = label.to_owned();
            assert!(
                declaration
                    .validate("config.toml", "native_checks[0]")
                    .is_ok(),
                "{operation:?} must retain {platform:?} support"
            );
        }
    }
    assert!(
        check(NativeHomebrewOperation::Install)
            .validate("config.toml", "native_checks[0]")
            .is_ok(),
        "native Homebrew install accepts hosted Apple ARM"
    );
}

#[test]
fn omitted_native_timeout_uses_five_minute_closed_default() {
    let mut value =
        serde_json::to_value(check(NativeHomebrewOperation::TapTree)).expect("serialize check");
    value
        .as_object_mut()
        .expect("object")
        .remove("timeout_minutes");
    let decoded: NativeHomebrewCheck = serde_json::from_value(value).expect("default timeout");
    assert_eq!(decoded.timeout_minutes, 5);
}
