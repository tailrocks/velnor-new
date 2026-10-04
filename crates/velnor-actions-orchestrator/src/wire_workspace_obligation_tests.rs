//! Workspace formatting carries the same mandatory original report frame.
use super::*;

fn proposal() -> ProposedTask {
    let group = velnor_actions_rust::TaskGroup {
        task_id: "stack/rust/root/fmt/default".to_owned(),
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: "root".to_owned(),
        kind: velnor_actions_rust::TaskKind::Fmt,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: velnor_actions_rust::CompileDriver::Cargo,
        test_runner: velnor_actions_rust::TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
    };
    velnor_actions_rust::propose_task(&group).expect("source adapter fmt proposal")
}

#[test]
fn workspace_format_frame_matches_original_runner_bound_task_digest() {
    let task = proposal();
    let catalog = ToolCatalog::pinned();
    let label = "ubuntu-26.04";
    let (argv, frame) = workspace_format_identity(&task, &catalog, label).expect("frame");
    let original = crate::vectors::task_argv_for_runner(&task, &catalog, label).expect("argv");
    let toolchain = crate::internal_plan::toolchain_id_for_runner(&task, &catalog, label)
        .expect("runner toolchain");
    let digest = crate::internal::plan_obligation::task_digest(
        &task.task_id,
        &original,
        &toolchain,
        None,
        None,
    )
    .expect("canonical original task digest");
    assert_eq!(argv, original);
    assert_eq!(
        frame[crate::matrix_step::OBLIGATION_TASK_ID_ENV],
        task.task_id
    );
    assert_eq!(
        frame[crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV],
        digest
    );
    let id = velnor_actions_contract::matrix_id_for_task_group(&task.stack_id, &task.task_id)
        .expect("matrix id");
    assert_eq!(frame[crate::matrix_step::OBLIGATION_MATRIX_ID_ENV], id);
    assert_eq!(
        frame[crate::matrix_step::OBLIGATION_MATRIX_KEY_ENV],
        matrix_key_for(&task).expect("key")
    );
    assert!(
        crate::helper_obligation_binding::binding_for_proposal(
            &task,
            &catalog,
            env!("CARGO_PKG_VERSION"),
            label,
        )
        .expect("helper selection")
        .is_none(),
        "fmt never invents a helper digest preimage"
    );
}

#[test]
fn workspace_format_identity_refuses_unknown_runner_instead_of_host_fallback() {
    assert!(
        workspace_format_identity(&proposal(), &ToolCatalog::pinned(), "unknown-host").is_err()
    );
}
