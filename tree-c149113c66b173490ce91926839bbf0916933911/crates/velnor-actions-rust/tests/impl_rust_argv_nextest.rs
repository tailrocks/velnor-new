//! Nextest build-preparation payload cases.

use super::impl_rust_argv::{group, profiled, text};
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::TaskKind;
use velnor_actions_rust::{NextestProfile, TestRunner};

#[test]
fn test_build_prepares_nextest_binaries_without_running_tests() -> Result<(), ContractError> {
    let mut build = group(TaskKind::Build);
    build.test_runner = TestRunner::CargoNextest;
    build.nextest_profile = NextestProfile::Ci;
    build.features = vec!["serde".to_owned()];
    build.target = "x86_64-unknown-linux-gnu".to_owned();
    assert_eq!(
        profiled(&build)?,
        [
            "nextest",
            "list",
            "--profile",
            "ci",
            "--list-type",
            "binaries-only",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
            "--no-default-features",
            "--features",
            "serde",
            "--target",
            "x86_64-unknown-linux-gnu",
        ]
    );
    assert_eq!(text(&group(TaskKind::Build))?[..2], ["test", "--no-run"]);
    Ok(())
}
