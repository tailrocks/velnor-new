//! Compiler report wrappers retain the actual emitted source registry records.

use super::*;
use velnor_actions_contract::StepKind;
use velnor_actions_rust::TaskKind;

#[test]
fn compiler_obligation_emits_one_exact_registered_report_wrapper() {
    let task = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let built = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![task]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        4,
    )
    .expect("compiled job");
    assert_eq!(built.helper_records.len(), 1);
    let record = &built.helper_records[0];
    let step = built.jobs[0]
        .1
        .steps
        .iter()
        .find(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
            if invocation == record.invocation() && env == record.environment())
        })
        .expect("exact registered compiler wrapper");
    assert!(
        step.condition
            .as_deref()
            .expect("uncovered gate")
            .starts_with("success()")
    );
    assert!(
        !built.jobs[0]
            .1
            .steps
            .iter()
            .any(|step| step.name.starts_with("Begin "))
    );
}

#[test]
fn rust_format_keeps_ordinary_shell_without_compiler_claim() {
    let task = crate_jobs_tests::group("demo", TaskKind::Fmt, &[]);
    let built = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![task.clone()]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        4,
    )
    .expect("format job");
    assert!(built.helper_records.is_empty());
    let name = step_name_for(&task.task_kind, &task.task_id);
    let step = built.jobs[0]
        .1
        .steps
        .iter()
        .find(|step| step.name == name)
        .expect("format");
    assert!(matches!(step.kind, StepKind::Shell { .. }));
}

pub(super) fn mbx_task() -> ProposedTask {
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TestRunner};
    let group = TaskGroup {
        task_id: "stack/rust/root/clippy/default".into(),
        package_id: "demo 0.1.0".into(),
        package_name: "demo".into(),
        manifest_key: "root".into(),
        kind: TaskKind::Clippy,
        configuration: "default".into(),
        features: Vec::new(),
        target: "host".into(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    };
    velnor_actions_rust::propose_task(&group).expect("adapter MBX proposal")
}

fn mbx_build() -> CrateBuild {
    build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![mbx_task()]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        4,
    )
    .expect("owner-produced MBX compiler wrapper")
}

fn object_transport() -> Step {
    velnor_actions_workflow_renderer::steps::mbx_objects_step(
        &format!(
            "jdx/mr-boxington-action@{}",
            velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_SHA
        ),
        false,
        velnor_actions_mise::MR_BOXINGTON_VERSION,
    )
    .expect("pinned object transport")
}

#[test]
fn original_mbx_wrapper_accepts_cold_and_one_warm_transport_rejects_two() {
    let built = mbx_build();
    assert_eq!(built.helper_records.len(), 1);
    assert_eq!(
        built.helper_records[0].compiler_driver(),
        Some(CompilerDriver::Mbx)
    );
    let mut jobs: BTreeMap<String, Job> = built.jobs.into_iter().collect();
    velnor_actions_workflow_renderer::steps::check_mbx_gating(
        &jobs,
        &built.drivers,
        &built.helper_records,
    )
    .expect("cold MBX retains selected compiler");
    let id = built.drivers.keys().next().expect("MBX job");
    jobs.get_mut(id)
        .expect("job")
        .steps
        .insert(0, object_transport());
    velnor_actions_workflow_renderer::steps::check_mbx_gating(
        &jobs,
        &built.drivers,
        &built.helper_records,
    )
    .expect("one warm object transport");
    jobs.get_mut(id)
        .expect("job")
        .steps
        .insert(0, object_transport());
    assert!(
        velnor_actions_workflow_renderer::steps::check_mbx_gating(
            &jobs,
            &built.drivers,
            &built.helper_records,
        )
        .is_err()
    );
}

#[test]
fn original_mbx_wrapper_rejects_missing_record_or_cargo_selection() {
    let built = mbx_build();
    let jobs: BTreeMap<String, Job> = built.jobs.into_iter().collect();
    assert!(
        velnor_actions_workflow_renderer::steps::check_mbx_gating(&jobs, &built.drivers, &[],)
            .is_err()
    );
    let cargo = built
        .drivers
        .keys()
        .map(|id| (id.clone(), CompilerDriver::Cargo))
        .collect();
    assert!(
        velnor_actions_workflow_renderer::steps::check_mbx_gating(
            &jobs,
            &cargo,
            &built.helper_records,
        )
        .is_err()
    );
}

#[test]
fn original_mbx_wrapper_rejects_invocation_or_environment_tampering() {
    let built = mbx_build();
    let mut jobs: BTreeMap<String, Job> = built.jobs.into_iter().collect();
    let id = built.drivers.keys().next().expect("MBX job");
    let index = jobs[id]
        .steps
        .iter()
        .position(|step| matches!(step.kind, StepKind::SourceBoundHelper { .. }))
        .expect("actual compiler step");
    let original = jobs[id].steps[index].clone();
    let StepKind::SourceBoundHelper { env, .. } =
        &mut jobs.get_mut(id).expect("job").steps[index].kind
    else {
        panic!("source helper expected");
    };
    env.insert(
        crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV.to_owned(),
        format!("b3-{}", "b".repeat(64)),
    );
    assert!(
        velnor_actions_workflow_renderer::steps::check_mbx_gating(
            &jobs,
            &built.drivers,
            &built.helper_records,
        )
        .is_err()
    );
    jobs.get_mut(id).expect("job").steps[index] = original;
    let StepKind::SourceBoundHelper { invocation, .. } =
        &mut jobs.get_mut(id).expect("job").steps[index].kind
    else {
        panic!("source helper expected");
    };
    let mut wire = serde_json::to_value(&*invocation).expect("invocation wire");
    wire["args"][0] = serde_json::json!("substituted-compiler-argument");
    *invocation = serde_json::from_value(wire).expect("structurally valid tamper");
    assert!(
        velnor_actions_workflow_renderer::steps::check_mbx_gating(
            &jobs,
            &built.drivers,
            &built.helper_records,
        )
        .is_err()
    );
}

#[test]
fn original_mbx_recipe_rejects_later_cargo_proposal_frame() {
    let original = mbx_task();
    let catalog = ToolCatalog::pinned();
    let recipe = crate::rust_report_wrapper::RustReportWrapper::from_proposal(
        &original,
        &catalog,
        "ubuntu-26.04",
    )
    .expect("source factory")
    .expect("MBX compiler recipe");
    let cargo = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let (obligations, _) =
        compile_obligations(&[&cargo], &catalog, "ubuntu-26.04").expect("new cargo proposal");
    assert!(
        recipe
            .bind_frame(&obligations[0], &catalog, &[], None)
            .is_err()
    );
}
