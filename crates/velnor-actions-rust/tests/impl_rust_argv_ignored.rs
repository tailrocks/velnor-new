//! Nextest ignored-test argv behavior.

use super::*;

#[test]
fn nextest_payload_carries_run_ignored() -> Result<(), ContractError> {
    let mut group = group(TaskKind::Nextest);
    group.test_runner = TestRunner::CargoNextest;
    group.run_ignored = Some("all".to_owned());
    let argv = text(&group)?;
    assert!(
        argv.windows(2).any(|window| window == ["--run-ignored", "all"]),
        "must contain --run-ignored all: {argv:?}"
    );

    let mut default_group = group(TaskKind::Nextest);
    default_group.test_runner = TestRunner::CargoNextest;
    default_group.run_ignored = Some("default".to_owned());
    let default_argv = text(&default_group)?;
    assert!(
        !default_argv.iter().any(|arg| arg == "--run-ignored"),
        "default mode must not emit --run-ignored: {default_argv:?}"
    );

    Ok(())
}
