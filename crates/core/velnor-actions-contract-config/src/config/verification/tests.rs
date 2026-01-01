use super::{
    VerificationRunner, VerificationTask, VerificationTaskKind, is_valid_mise_task_name,
    is_valid_verification_task_id,
};
use crate::config::ArtifactBuildOutput;

fn task(id: &str, mise_task: &str, timeout_minutes: u16) -> VerificationTask {
    VerificationTask {
        id: id.to_owned(),
        kind: VerificationTaskKind::Verification,
        mise_task: mise_task.to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes,
        outputs: Vec::new(),
    }
}

#[test]
fn verification_outputs_reuse_the_bounded_inventory_contract() {
    let mut candidate = task("frontend", "build-frontend", 30);
    candidate.outputs = vec![ArtifactBuildOutput {
        id: "bundle".to_owned(),
        path: "dist/app.tar".to_owned(),
        max_bytes: 16_384,
    }];
    assert!(candidate.validate("config.toml").is_ok());

    candidate.outputs[0].path = "dist/../secret".to_owned();
    let error = candidate
        .validate("config.toml")
        .expect_err("unsafe output fails closed")
        .to_string();
    assert!(error.contains("workflow.tasks.outputs.path"));
    assert!(error.contains("unsafe_artifact_path"));
}

#[test]
fn verification_outputs_are_linux_x64_only_and_empty_is_omitted() {
    let mut candidate = task("desktop", "desktop-test", 20);
    candidate.outputs = vec![ArtifactBuildOutput {
        id: "bundle".to_owned(),
        path: "dist/app.tar".to_owned(),
        max_bytes: 16_384,
    }];
    candidate.runner = VerificationRunner::MacosArm64;
    assert!(
        candidate
            .validate("config.toml")
            .expect_err("macOS output capture is unsupported")
            .to_string()
            .contains("artifact_build_requires_linux_x64")
    );

    let empty = task("audit", "audit", 20);
    let serialized = serde_json::to_value(&empty).expect("task serializes");
    assert!(serialized.get("outputs").is_none());
}

#[test]
fn ids_and_mise_task_names_reject_shell_and_yaml_syntax() {
    for id in ["native-swift-format", "check1", "a-b-c"] {
        assert!(is_valid_verification_task_id(id), "{id}");
    }
    for id in ["", "Upper", "-start", "end-", "a--b", "required", "x/y"] {
        assert!(!is_valid_verification_task_id(id), "{id:?}");
    }
    for name in ["audit", "lint:strict", "tool_1.test"] {
        assert!(is_valid_mise_task_name(name), "{name}");
    }
    for name in ["", "--flag", "a b", "a/b", "${{ x }}", "a;id"] {
        assert!(!is_valid_mise_task_name(name), "{name:?}");
    }
}

#[test]
fn timeout_is_bounded_and_runner_targets_are_platform_specific() {
    for timeout in [1, 360] {
        assert!(
            task("audit", "audit", timeout)
                .validate("config.toml")
                .is_ok()
        );
    }
    for timeout in [0, 361, u16::MAX] {
        assert!(
            task("audit", "audit", timeout)
                .validate("config.toml")
                .is_err()
        );
    }
    assert_eq!(
        VerificationRunner::LinuxX64.mise_target(),
        "x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        VerificationRunner::MacosArm64.mise_target(),
        "aarch64-apple-darwin"
    );
}
