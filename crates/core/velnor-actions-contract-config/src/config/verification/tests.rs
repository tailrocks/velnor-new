use super::{
    VerificationRunner, VerificationTask, VerificationTaskKind, is_valid_mise_task_name,
    is_valid_verification_task_id,
};

fn task(id: &str, mise_task: &str, timeout_minutes: u16) -> VerificationTask {
    VerificationTask {
        id: id.to_owned(),
        kind: VerificationTaskKind::Verification,
        mise_task: mise_task.to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes,
    }
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
