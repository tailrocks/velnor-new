use super::{SystemToolProof, validate_system_tool_proofs};
use std::path::Path;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, CheckSystemTool, CheckSystemToolKind, MiseCheck,
};

#[test]
fn maximum_valid_swift_observation_fits_the_declared_receipt_bound() {
    let version = maximum_version();
    let clang = format!("{}9", "9.".repeat(55));
    let build = format!("swiftlang-{version} clang-{clang}");
    assert_eq!(build.len(), 256);
    let pin = CheckSystemTool {
        kind: CheckSystemToolKind::Swift,
        version: version.clone(),
        build: build.clone(),
    };
    let path = format!("/{}", "\u{1}".repeat(1023));
    let digest = velnor_actions_contract::digest_b3(b"system-proof");
    let proof = SystemToolProof {
        declared: pin.clone(),
        observed_version: version.clone(),
        observed_build: build,
        observed_target: Some(format!("arm64-apple-macosx{version}")),
        executable: path.clone(),
        launcher: "/usr/bin/xcrun".into(),
        developer_dir: path,
        developer_stdout_digest: digest.clone(),
        developer_stderr_digest: digest.clone(),
        stdout_digest: digest.clone(),
        stderr_digest: digest.clone(),
        discovery_stdout_digest: Some(digest.clone()),
        discovery_stderr_digest: Some(digest),
    };
    validate_system_tool_proofs(
        CheckPlatform::MacosArm64,
        std::slice::from_ref(&pin),
        std::slice::from_ref(&proof),
    )
    .expect("maximal proof remains valid");
    let check = MiseCheck {
        id: "native".into(),
        task: "verify".into(),
        directory: ".".into(),
        runner: CheckRunner {
            label: "macos-15".into(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: Vec::new(),
        tools: Vec::new(),
        system_tools: vec![pin],
        evidence: None,
        timeout_minutes: 10,
    };
    let proof_bytes = serde_json::to_vec(&proof).expect("proof JSON").len();
    let bound = velnor_actions_contract::check_execution_receipt_upper_bound(&check, &[])
        .expect("receipt budget");
    assert!(bound >= 32 * 1024 + proof_bytes);
    assert!(bound <= velnor_actions_contract::MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    assert!(Path::new(&proof.developer_dir).is_absolute());
}

fn maximum_version() -> String {
    format!("{}99", "9.".repeat(63))
}
