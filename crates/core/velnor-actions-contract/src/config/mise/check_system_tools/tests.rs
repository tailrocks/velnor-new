use super::*;

fn swift() -> CheckSystemTool {
    CheckSystemTool {
        kind: CheckSystemToolKind::Swift,
        version: "6.2.1".to_owned(),
        build: "swiftlang-6.2.1.1.1".to_owned(),
    }
}

#[test]
fn system_pins_require_macos_safe_values_and_unique_sorted_kinds() {
    let tools = vec![swift()];
    assert!(
        validate_system_tools(&tools, CheckPlatform::MacosArm64, "config", "checks[0]").is_ok()
    );
    assert!(validate_system_tools(&tools, CheckPlatform::LinuxX64, "config", "checks[0]").is_err());
    let duplicate = vec![swift(), swift()];
    assert!(
        validate_system_tools(&duplicate, CheckPlatform::MacosX64, "config", "checks[0]").is_err()
    );
    for bad in [
        "",
        "$(command)",
        "${{ env.X }}",
        "-flag",
        "a b",
        "a\nb",
        "https://example.com",
    ] {
        let mut invalid = swift();
        invalid.version = bad.to_owned();
        assert!(
            validate_system_tools(&[invalid], CheckPlatform::MacosArm64, "config", "checks[0]")
                .is_err(),
            "{bad}"
        );
    }
    let mut xcode = swift();
    xcode.kind = CheckSystemToolKind::Xcode;
    assert!(
        validate_system_tools(
            &[xcode.clone(), swift()],
            CheckPlatform::MacosArm64,
            "config",
            "checks[0]"
        )
        .is_err()
    );
    assert!(
        validate_system_tools(
            &[swift(), xcode],
            CheckPlatform::MacosArm64,
            "config",
            "checks[0]"
        )
        .is_ok()
    );
}
#[test]
fn complete_swift_compiler_tuple_is_pinned_without_shell_syntax() {
    let mut pin = swift();
    pin.version = "6.4".to_owned();
    pin.build = "swiftlang-6.4.0.34.1 clang-2100.3.34.1".to_owned();
    assert!(
        validate_system_tools(
            &[pin.clone()],
            CheckPlatform::MacosArm64,
            "config",
            "checks[0]"
        )
        .is_ok()
    );
    for build in [
        "",
        " leading",
        "trailing ",
        "double  spaces",
        "a\nb",
        "${{ env.X }}",
        "$(id)",
        "a;b",
        "a|b",
        "a\tb",
    ] {
        pin.build = build.to_owned();
        assert!(
            validate_system_tools(
                &[pin.clone()],
                CheckPlatform::MacosArm64,
                "config",
                "checks[0]"
            )
            .is_err(),
            "{build:?}"
        );
    }
    assert!(!safe_build(&"a".repeat(257)));
    for version in ["", ".6", "6.", "6..4", "swift-6.4", "6 4", "6;4"] {
        assert!(!safe_version(version), "{version:?}");
    }
}
