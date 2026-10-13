use velnor_actions_contract::canonical::digest_b3;
use velnor_actions_contract::config::{CheckPlatform, CheckSystemTool, CheckSystemToolKind};
use velnor_actions_mise_core::checks::{
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

fn pin() -> CheckSystemTool {
    CheckSystemTool {
        kind: CheckSystemToolKind::Swift,
        version: "6.4".to_owned(),
        build: "swiftlang-6.4.0.34.1 clang-2100.3.34.1".to_owned(),
    }
}

fn proof() -> SystemToolProof {
    SystemToolProof { declared: pin(), observed_version: "6.4".to_owned(),
        observed_build: pin().build, observed_target: Some("arm64-apple-macosx27.0.0".to_owned()),
        executable: "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift-frontend".to_owned(),
        launcher: "/usr/bin/xcrun".to_owned(), stdout_digest: digest_b3(SWIFT.as_bytes()),
        developer_dir: "/Applications/Xcode.app/Contents/Developer".to_owned(),
        developer_stdout_digest: digest_b3(b"/Applications/Xcode.app/Contents/Developer\n"),
        developer_stderr_digest: digest_b3(b""),
        stderr_digest: digest_b3(b""), discovery_stdout_digest: Some(digest_b3(b"/swift\n")),
        discovery_stderr_digest: Some(digest_b3(b"")) }
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
    let deadline =
        velnor_actions_mise_core::CheckDeadline::after(std::time::Duration::from_secs(60))
            .expect("test deadline");
    assert!(verify_check_system_tools(CheckPlatform::LinuxX64, &[pin()], deadline).is_err());
    assert!(
        verify_check_system_tools(
            CheckPlatform::LinuxX64,
            &[],
            velnor_actions_mise_core::CheckDeadline::after(std::time::Duration::from_secs(60))
                .expect("test deadline"),
        )
        .expect("empty")
        .is_empty()
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
