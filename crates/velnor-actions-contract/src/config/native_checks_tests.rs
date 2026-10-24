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
