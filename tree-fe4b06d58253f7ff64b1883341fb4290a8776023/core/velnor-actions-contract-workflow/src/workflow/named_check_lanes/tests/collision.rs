use super::super::*;
use super::{config, ir, runner};
use crate::expand_workflow;
use velnor_actions_contract_config::config::CheckPlatform;

#[test]
fn expanded_job_id_collision_fails_instead_of_replacing_a_check() {
    let source = ir(&[
        ("check-foo", runner("ubuntu-26.04", CheckPlatform::LinuxX64)),
        (
            "check-foo__hosted",
            runner("macos-15", CheckPlatform::MacosArm64),
        ),
    ]);
    let error = expand_workflow(&source, &config(ExecutionMode::Both), None)
        .expect_err("both outputs collide with the fixed macOS check");
    assert!(
        error
            .to_string()
            .contains("expanded job id check-foo__hosted"),
        "{error}"
    );
}
