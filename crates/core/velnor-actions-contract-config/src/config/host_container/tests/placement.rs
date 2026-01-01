use super::{admitted, docker, orb};
use crate::config::{CheckExecutor, CheckPlatform};

#[test]
fn installed_container_placement_is_explicit() {
    assert!(admitted(
        &docker(),
        CheckPlatform::LinuxX64,
        CheckExecutor::Hosted
    ));
    assert!(admitted(
        &docker(),
        CheckPlatform::LinuxX64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(admitted(
        &docker(),
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(!admitted(
        &docker(),
        CheckPlatform::MacosArm64,
        CheckExecutor::Hosted
    ));
    assert!(admitted(
        &orb(),
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(!admitted(
        &orb(),
        CheckPlatform::LinuxX64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(!admitted(
        &orb(),
        CheckPlatform::MacosArm64,
        CheckExecutor::Hosted
    ));
}
