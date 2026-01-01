//! Actual receipt identity is distinct from exact declaration pins.
use super::*;
use velnor_actions_mise::check_tool_probes::QualifiedExecutableObservation;

fn fixture() -> (QualifiedTool, QualifiedToolReceipt) {
    let tool: QualifiedTool = serde_json::from_value(serde_json::json!({
        "id":"node","backend":{"kind":"core","tool":"node"},"version":"24.1.0",
        "options":{"kind":"default"},"depends_on":[],"platforms":[{
            "platform":"linux_x64","artifacts":[{"url":"https://nodejs.org/dist/v24.1.0/node-v24.1.0-linux-x64.tar.xz","sha256":"a".repeat(64)}],
            "dependency_artifacts":[],"install_tree_sha256":"b".repeat(64),
            "executables":[{"name":"node","path":"bin/node","sha256":"c".repeat(64),
                "probe":{"kind":"version","expected":"node 24.1.0"}}]}]
    })).expect("tool");
    let platform = &tool.platforms[0];
    let stdout = "node 24.1.0\n".to_owned();
    let executable = QualifiedExecutableProof {
        tool_id: tool.id.clone(),
        tool_version: tool.version.clone(),
        platform: CheckPlatform::LinuxX64,
        declared: platform.executables[0].clone(),
        observed: QualifiedExecutableObservation {
            name: "node".into(),
            path: "/tmp/owned/tools/node/prefix/bin/node".into(),
            sha256: "c".repeat(64),
        },
        stdout_digest: digest_b3(stdout.as_bytes()),
        stdout,
        stderr_digest: digest_b3(b""),
    };
    let proof = receipt(
        &tool,
        CheckPlatform::LinuxX64,
        platform.artifacts.clone(),
        vec![],
        platform.install_tree_sha256.clone(),
        vec![executable],
    )
    .expect("proof");
    (tool, proof)
}

#[test]
fn exact_observations_pass_but_missing_and_extra_closure_members_fail() {
    let (tool, proof) = fixture();
    assert!(validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&proof)
    ));
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        &[]
    ));
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        &[tool],
        &[proof.clone(), proof]
    ));
}

#[test]
fn artifact_tree_executable_definition_version_and_path_drift_fail() {
    let (tool, proof) = fixture();
    for mode in 0..7 {
        let mut changed = proof.clone();
        match mode {
            0 => changed.artifacts[0].sha256 = "d".repeat(64),
            1 => changed.install_tree_sha256 = "d".repeat(64),
            2 => changed.executables[0].observed.sha256 = "d".repeat(64),
            3 => changed.definition_digest = digest_b3(b"foreign definition"),
            4 => changed.executables[0].stdout = "node 24.2.0".into(),
            5 => changed.executables[0].observed.path = "/tmp/foreign/node".into(),
            _ => changed.platform = CheckPlatform::MacosArm64,
        }
        assert!(
            !validate_receipts(
                CheckPlatform::LinuxX64,
                std::slice::from_ref(&tool),
                &[changed]
            ),
            "{mode}"
        );
    }
}
