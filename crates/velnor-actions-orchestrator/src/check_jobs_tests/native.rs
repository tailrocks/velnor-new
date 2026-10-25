//! Native Homebrew check job shape.

use super::*;

use velnor_actions_contract::config::{NativeHomebrewCheck, NativeHomebrewOperation};

#[test]
fn native_check_job_uses_closed_sdk_step_and_standard_plan_transport() {
    let check = NativeHomebrewCheck {
        id: "tap-tree".to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        formula_channel: None,
        runner: hosted("macos-15", CheckPlatform::MacosArm64),
        timeout_minutes: 1,
    };
    let discovery = Discovery {
        native_checks: vec![check],
        ..Discovery::default()
    };
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("native check job");
    assert_eq!(jobs.len(), 1);
    let (id, job) = &jobs[0];
    assert_eq!(id, "check-tap-tree");
    assert_eq!(job.needs, ["plan"]);
    let permissions = job.permissions.as_ref().expect("read-only API permissions");
    assert_eq!(
        permissions.contents,
        velnor_actions_contract::workflow::permissions::PermissionLevel::Read
    );
    assert_eq!(
        permissions.actions,
        velnor_actions_contract::workflow::permissions::PermissionLevel::Read
    );
    assert_eq!(job.steps[0].name, "Checkout");
    let prepare = job
        .steps
        .iter()
        .find(|step| step.name == "Prepare pinned tools")
        .expect("pinned gh preparation");
    let StepKind::Shell { run, .. } = &prepare.kind else {
        panic!("pinned gh preparation");
    };
    assert!(run.join(" ").contains("gh@"));
    let step = job
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::NativeCheckExecution { .. }))
        .expect("typed native execution");
    let StepKind::NativeCheckExecution {
        check_id,
        operation,
        task_id,
        ..
    } = &step.kind
    else {
        panic!("native check execution variant");
    };
    assert_eq!(check_id, "tap-tree");
    assert_eq!(*operation, NativeHomebrewOperation::TapTree);
    assert_eq!(task_id, "internal/native-check/tap-tree");
    assert_eq!(
        job.steps.last().and_then(|step| step.condition.as_deref()),
        Some("always()")
    );
}
