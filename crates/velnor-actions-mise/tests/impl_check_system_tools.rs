use velnor_actions_contract::canonical::digest_b3;
use velnor_actions_contract::config::{
    CheckPlatform, CheckSystemTool, CheckSystemToolKind, is_valid_check_system_build,
    is_valid_check_system_version,
};
use velnor_actions_mise::checks::{
    SystemToolProof, parse_system_tool_version, validate_system_tool_proofs,
    verify_check_system_tools,
};

const SWIFT: &str = "swift-driver version: 1.168.6 Apple Swift version 6.4 (swiftlang-6.4.0.34.1 clang-2100.3.34.1)\nTarget: arm64-apple-macosx27.0.0\n";

#[test]
fn real_native_output_grammar_preserves_exact_build_identity() {
    assert_eq!(
        parse_system_tool_version(
            CheckSystemToolKind::Xcode,
            b"Xcode 27.0\nBuild version 27A266a\n",
            b""
        )
        .expect("xcode"),
        ("27.0".to_owned(), "27A266a".to_owned())
    );
    assert_eq!(
        parse_system_tool_version(
            CheckSystemToolKind::Xcode,
            b"Xcode 27.1\nBuild version 27A9275\n",
            b""
        )
        .expect("qualified Xcode 27.1"),
        ("27.1".to_owned(), "27A9275".to_owned())
    );
    assert_eq!(
        parse_system_tool_version(CheckSystemToolKind::Swift, SWIFT.as_bytes(), b"")
            .expect("swift"),
        (
            "6.4".to_owned(),
            "swiftlang-6.4.0.34.1 clang-2100.3.34.1".to_owned()
        )
    );
    assert!(
        parse_system_tool_version(
            CheckSystemToolKind::Swift,
            SWIFT
                .trim_start_matches("swift-driver version: 1.168.6 ")
                .as_bytes(),
            b""
        )
        .is_ok()
    );
}

#[test]
fn malformed_negative_nonapple_nonutf8_and_diagnostic_output_fail_closed() {
    for output in [
        "",
        "Xcode -27.0\nBuild version 27A266a\n",
        "Xcode 27..0\nBuild version 27A266a\n",
        "Xcode 27.0\nBuild version -27A266a\n",
        "Xcode 27.0\nBuild version 27A266a\nextra\n",
    ] {
        assert!(
            parse_system_tool_version(CheckSystemToolKind::Xcode, output.as_bytes(), b"").is_err()
        );
    }
    for output in [
        SWIFT.replace("Apple Swift", "Swift"),
        SWIFT.replace("6.4 (", "-6.4 ("),
        SWIFT.replace("swiftlang-", "unknown-"),
        SWIFT.replace("clang-2100.3.34.1", "clang--2100.3.34.1"),
        SWIFT.replace("arm64-apple-macosx27.0.0", "aarch64-unknown-linux-gnu"),
        format!("{SWIFT}unexpected\n"),
    ] {
        assert!(
            parse_system_tool_version(CheckSystemToolKind::Swift, output.as_bytes(), b"").is_err()
        );
    }
    assert!(parse_system_tool_version(CheckSystemToolKind::Swift, &[0xff], b"").is_err());
    assert!(
        parse_system_tool_version(CheckSystemToolKind::Swift, SWIFT.as_bytes(), &[0xff]).is_err()
    );
    assert!(
        parse_system_tool_version(CheckSystemToolKind::Swift, SWIFT.as_bytes(), b"warning")
            .is_err()
    );
}

#[test]
fn runtime_parser_and_shared_schema_agree_on_version_and_build_grammar() {
    for version in ["27.0", "27.1", "27", "027.0", "27.00"] {
        let output = format!("Xcode {version}\nBuild version 27A9275\n");
        assert_eq!(
            parse_system_tool_version(CheckSystemToolKind::Xcode, output.as_bytes(), b"").is_ok(),
            is_valid_check_system_version(version),
            "Xcode version {version}"
        );
    }
    for build in ["27A9275", "27.0", "27A-9275", "27.1 RC"] {
        let output = format!("Xcode 27.1\nBuild version {build}\n");
        assert_eq!(
            parse_system_tool_version(CheckSystemToolKind::Xcode, output.as_bytes(), b"").is_ok(),
            is_valid_check_system_build(CheckSystemToolKind::Xcode, build),
            "Xcode build {build}"
        );
    }
    for build in [
        "swiftlang-6.4.0.34.1 clang-2100.3.34.1",
        "swiftlang-6.2.1.1.1",
        "swiftlang-6.4 clang-2100.3 bad",
    ] {
        let output =
            format!("Apple Swift version 6.4 ({build})\nTarget: arm64-apple-macosx27.0.0\n");
        assert_eq!(
            parse_system_tool_version(CheckSystemToolKind::Swift, output.as_bytes(), b"").is_ok(),
            is_valid_check_system_build(CheckSystemToolKind::Swift, build),
            "Swift build {build}"
        );
    }
}

fn pin() -> CheckSystemTool {
    CheckSystemTool {
        kind: CheckSystemToolKind::Swift,
        version: "6.4".to_owned(),
        build: "swiftlang-6.4.0.34.1 clang-2100.3.34.1".to_owned(),
        sdk_version: None,
        developer_dir: None,
    }
}

fn proof() -> SystemToolProof {
    SystemToolProof { declared: pin(), observed_version: "6.4".to_owned(), observed_sdk_version: None,
        sdk_stdout_digest: None, sdk_stderr_digest: None,
        observed_build: pin().build, observed_target: Some("arm64-apple-macosx27.0.0".to_owned()),
        executable: "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift-frontend".to_owned(),
        launcher: "/usr/bin/xcrun".to_owned(), stdout_digest: digest_b3(SWIFT.as_bytes()),
        developer_dir: "/Applications/Xcode.app/Contents/Developer".to_owned(),
        developer_stdout_digest: digest_b3(b"/Applications/Xcode.app/Contents/Developer\n"),
        developer_stderr_digest: digest_b3(b""),
        stderr_digest: digest_b3(b""), discovery_stdout_digest: Some(digest_b3(b"/swift\n")),
        discovery_stderr_digest: Some(digest_b3(b"")) }
}

fn xcode_pin() -> CheckSystemTool {
    CheckSystemTool {
        kind: CheckSystemToolKind::Xcode,
        version: "27.1".to_owned(),
        build: "27A9275".to_owned(),
        sdk_version: Some("27.0".to_owned()),
        developer_dir: Some(
            "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer".to_owned(),
        ),
    }
}

fn xcode_proof(
    pin: CheckSystemTool,
    version: &str,
    build: &str,
    sdk: &str,
    directory: &str,
) -> SystemToolProof {
    let digest = digest_b3(b"xcode-evidence");
    SystemToolProof {
        declared: pin,
        observed_version: version.to_owned(),
        observed_build: build.to_owned(),
        observed_sdk_version: Some(sdk.to_owned()),
        sdk_stdout_digest: Some(digest.clone()),
        sdk_stderr_digest: Some(digest.clone()),
        observed_target: None,
        executable: "/usr/bin/xcodebuild".to_owned(),
        launcher: "/usr/bin/xcodebuild".to_owned(),
        developer_dir: directory.to_owned(),
        developer_stdout_digest: digest.clone(),
        developer_stderr_digest: digest.clone(),
        stdout_digest: digest.clone(),
        stderr_digest: digest,
        discovery_stdout_digest: None,
        discovery_stderr_digest: None,
    }
}

#[test]
fn exact_xcode_pin_requires_configured_directory_version_build_and_sdk() {
    let directory = "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer";
    let pin = xcode_pin();
    let qualified = xcode_proof(pin.clone(), "27.1", "27A9275", "27.0", directory);
    assert!(
        validate_system_tool_proofs(
            CheckPlatform::MacosArm64,
            std::slice::from_ref(&pin),
            std::slice::from_ref(&qualified),
        )
        .is_ok()
    );

    let selected_27 = xcode_proof(
        pin.clone(),
        "27.0",
        "27A266a",
        "27.0",
        "/Applications/Xcode.app/Contents/Developer",
    );
    assert!(
        validate_system_tool_proofs(CheckPlatform::MacosArm64, &[pin], &[selected_27],).is_err()
    );
}

#[test]
fn receipts_require_exact_pins_paths_digests_and_platform() {
    let platform = CheckPlatform::MacosArm64;
    assert!(validate_system_tool_proofs(platform, &[pin()], &[proof()]).is_ok());
    assert!(validate_system_tool_proofs(platform, &[pin()], &[]).is_err());
    assert!(validate_system_tool_proofs(CheckPlatform::LinuxX64, &[pin()], &[proof()]).is_err());
    assert!(validate_system_tool_proofs(CheckPlatform::MacosX64, &[pin()], &[proof()]).is_err());
    for field in 0..10 {
        let mut invalid = proof();
        match field {
            0 => invalid.observed_version = "6.5".to_owned(),
            1 => invalid.observed_build.push('x'),
            2 => invalid.declared.version = "6.5".to_owned(),
            3 => invalid.executable = "relative/swift".to_owned(),
            4 => invalid.launcher = "/tmp/xcrun".to_owned(),
            5 => invalid.stdout_digest = "bad".to_owned(),
            6 => invalid.discovery_stderr_digest = None,
            7 => invalid.developer_dir = "/Applications/Other.app/Contents/Developer".to_owned(),
            8 => invalid.developer_stdout_digest = "bad".to_owned(),
            _ => invalid.observed_target = None,
        }
        assert!(validate_system_tool_proofs(platform, &[pin()], &[invalid]).is_err());
    }
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))
        .expect("test deadline");
    assert!(verify_check_system_tools(CheckPlatform::LinuxX64, &[pin()], deadline).is_err());
    assert_eq!(
        verify_check_system_tools(
            CheckPlatform::LinuxX64,
            &[],
            velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))
                .expect("test deadline"),
        )
        .expect("empty"),
        [] as [velnor_actions_mise::checks::SystemToolProof; 0]
    );
}

#[test]
fn closed_proof_wire_rejects_unknown_fields_and_undeclared_tools() {
    let mut value = serde_json::to_value(proof()).expect("proof");
    value
        .as_object_mut()
        .expect("object")
        .insert("argv".to_owned(), serde_json::json!(["bad"]));
    assert!(serde_json::from_value::<SystemToolProof>(value).is_err());
    assert!(validate_system_tool_proofs(CheckPlatform::MacosArm64, &[], &[proof()]).is_err());
    let mut value = serde_json::to_value(pin()).expect("pin");
    value["kind"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<CheckSystemTool>(value).is_err());
}
