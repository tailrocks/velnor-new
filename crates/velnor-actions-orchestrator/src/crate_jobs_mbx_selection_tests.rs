//! Cache qualification stays separate from selected compiler correctness.
use super::*;

#[test]
fn drivers_follow_per_crate_selection() {
    let mbx = crate::crate_jobs::source_helper_tests::mbx_task();
    let cargo = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![mbx, cargo]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(
        found.drivers["rust-demo"],
        velnor_actions_contract::CompilerDriver::Mbx
    );
    assert_eq!(
        found.drivers["rust-nested"],
        velnor_actions_contract::CompilerDriver::Cargo
    );
    let steps = names(&found.jobs[0].1);
    assert!(
        !steps.contains(&"Restore MBX objects"),
        "unqualified restore: {steps:?}"
    );
    assert!(
        steps.contains(&"Clippy"),
        "cold MBX still executes: {steps:?}"
    );
    let lint = found.jobs[0]
        .1
        .steps
        .iter()
        .find(|step| step.name == "Clippy")
        .expect("MBX obligation");
    let velnor_actions_contract::StepKind::SourceBoundHelper { invocation, env } = &lint.kind
    else {
        panic!("MBX obligation must carry its source-owned report wrapper");
    };
    let record = found
        .helper_records
        .iter()
        .find(|record| record.invocation() == invocation && record.environment() == env)
        .expect("exact compiler record retained");
    record.validate_binding().expect("exact source bytes bound");
    assert_eq!(
        record.compiler_driver(),
        Some(velnor_actions_contract::CompilerDriver::Mbx)
    );
    assert!(record.source().contains("mbx") && record.source().contains("clippy"));
    assert_eq!(
        env.get(crate::matrix_step::OBLIGATION_TASK_ID_ENV)
            .map(String::as_str),
        Some("stack/rust/root/clippy/default")
    );
    let steps = names(&found.jobs[1].1);
    assert!(!steps.contains(&"Restore MBX objects"), "{steps:?}");
}
