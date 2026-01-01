//! Tool receipts bind installed trees to exact qualified pins.

use velnor_actions_contract::digest_b3;
use velnor_actions_contract_config::config::{CheckPlatform, QualifiedTool};
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedExecutableProof,
};
use velnor_actions_orchestrator_check_acquisition::tools::{
    QualifiedToolReceipt, receipt, validate_receipts,
};

fn fixture() -> (QualifiedTool, QualifiedToolReceipt) {
    let tool: QualifiedTool = serde_json::from_value(serde_json::json!({
        "id":"node","backend":{"kind":"core","tool":"node"},"version":"24.1.0",
        "options":{"kind":"default"},"depends_on":[],"platforms":[{
            "platform":"linux_x64","artifacts":[{"url":"https://nodejs.org/dist/v24.1.0/node-v24.1.0-linux-x64.tar.xz","sha256":"a".repeat(64)}],
            "dependency_artifacts":[],"install_tree_sha256":"b".repeat(64),
            "executables":[{"name":"node","path":"bin/node","sha256":"c".repeat(64),
                "probe":{"kind":"version","expected":"node 24.1.0"}}]}]
    }))
    .expect("tool");
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
fn receipt_carries_declaration_identity() {
    let (tool, proof) = fixture();
    assert_eq!(proof.id, tool.id);
    assert_eq!(proof.version, tool.version);
    assert_eq!(proof.platform, CheckPlatform::LinuxX64);
    assert_eq!(
        proof.install_tree_sha256,
        tool.platforms[0].install_tree_sha256
    );
}

#[test]
fn validate_receipts_accepts_exact_closure() {
    let (tool, proof) = fixture();
    assert!(validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&proof)
    ));
}

#[test]
fn validate_receipts_rejects_missing_member() {
    let (tool, _) = fixture();
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        &[]
    ));
}

#[test]
fn validate_receipts_rejects_extra_member() {
    let (tool, proof) = fixture();
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        &[proof.clone(), proof]
    ));
}

#[test]
fn validate_receipts_rejects_artifact_and_tree_drift() {
    let (tool, proof) = fixture();
    let mut changed = proof.clone();
    changed.artifacts[0].sha256 = "d".repeat(64);
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&changed)
    ));
    changed = proof.clone();
    changed.install_tree_sha256 = "d".repeat(64);
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&changed)
    ));
}

#[test]
fn validate_receipts_rejects_observed_path_escape() {
    let (tool, proof) = fixture();
    let mut changed = proof.clone();
    changed.executables[0].observed.path = "/tmp/foreign/node".into();
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&changed)
    ));
}

#[test]
fn validate_receipts_rejects_definition_and_platform_drift() {
    let (tool, proof) = fixture();
    let mut changed = proof.clone();
    changed.definition_digest = digest_b3(b"foreign definition");
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&changed)
    ));
    changed = proof;
    changed.platform = CheckPlatform::MacosArm64;
    assert!(!validate_receipts(
        CheckPlatform::LinuxX64,
        std::slice::from_ref(&tool),
        std::slice::from_ref(&changed)
    ));
}
