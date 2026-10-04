//! Compiler handoff rejects mismatched, duplicate and unconsumed obligations.

use super::*;
use velnor_actions_contract::{HelperInvocation, SourceBoundHelper, SourceBoundOperation};
use velnor_actions_mise::ToolCatalog;

fn obligation() -> CrateObligation {
    CrateObligation {
        task_id: "stack/workload/app/build/desktop".into(),
        kind: "build".into(),
        step_name: "Prepare SDK".into(),
        gated_by: Vec::new(),
        matrix_key: "m-0123456789abcdef".into(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        run: vec!["original".into()],
    }
}

fn binding() -> ProposalHelperBinding {
    let operation = SourceBoundOperation::DesktopNativeHydration;
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let sha =
        velnor_actions_contract::workflow::source_helper::compiled_source_sha256(source.as_bytes());
    let helper = SourceBoundHelper::compiled(operation, operation.path(), &sha).expect("helper");
    let invocation =
        HelperInvocation::compiled(helper, vec!["sdk".into()], Vec::new()).expect("invocation");
    let record = CompiledSourceHelper::compiled(invocation, source).expect("record");
    let descriptor = HelperObligationDescriptor::from_compiled(&record, &obligation().matrix_key)
        .expect("descriptor");
    ProposalHelperBinding {
        native_recipe: None,
        record,
        descriptor,
    }
}

#[test]
fn exact_compiler_record_survives_and_duplicate_or_unused_records_fail() {
    let obligation = obligation();
    let binding = binding();
    let expected = binding.record.clone();
    let mut records = SourceBindings::default();
    records
        .insert_source_binding(&obligation, binding)
        .expect("insert");
    let Some(ExecutionRecipe::NativeHelper { record, .. }) =
        records.execution_for(&obligation).expect("lookup")
    else {
        panic!("native recipe expected");
    };
    assert_eq!(record.as_ref(), &expected);
    records
        .validate_coverage(std::slice::from_ref(&obligation))
        .expect("covered");
    assert!(
        records
            .insert_source_binding(&obligation, self::binding())
            .is_err()
    );
    assert!(records.validate_coverage(&[]).is_err());
    assert!(
        records
            .validate_coverage(&[obligation.clone(), obligation])
            .is_err()
    );
}

#[test]
fn substitution_of_digest_matrix_identity_argv_or_gates_fails() {
    let original = obligation();
    let mut records = SourceBindings::default();
    records
        .insert_source_binding(&original, binding())
        .expect("insert");
    let mut changed = original.clone();
    changed.task_digest = format!("b3-{}", "b".repeat(64));
    assert!(records.execution_for(&changed).is_err());
    changed = original.clone();
    changed.matrix_key = "m-fedcba9876543210".into();
    assert!(records.execution_for(&changed).is_err());
    changed = original.clone();
    changed.run = vec!["replacement".into()];
    assert!(records.execution_for(&changed).is_err());
    changed = original;
    changed
        .gated_by
        .push("stack/workload/app/check/desktop".into());
    assert!(records.execution_for(&changed).is_err());
}

#[test]
fn closed_helper_owner_cannot_fall_back_without_compiler_record() {
    let mut obligation = obligation();
    obligation.task_id = "stack/tofu/app/init/default".into();
    assert!(
        SourceBindings::default()
            .execution_for(&obligation)
            .is_err()
    );
}

#[test]
fn descriptor_must_match_exact_compiler_record() {
    let mut binding = binding();
    binding.descriptor =
        HelperObligationDescriptor::from_compiled(&binding.record, "m-fedcba9876543210")
            .expect("descriptor");
    assert!(
        SourceBindings::default()
            .insert_source_binding(&obligation(), binding)
            .is_err()
    );
}

#[test]
fn compiler_owner_cannot_fall_back_without_original_proposal_recipe() {
    let mut obligation = obligation();
    obligation.task_id = "stack/rust/app/clippy/default".into();
    assert!(
        SourceBindings::default()
            .execution_for(&obligation)
            .is_err()
    );
}

#[test]
fn compiler_recipe_survives_before_frame_binding_and_rejects_model_substitution() {
    let task = crate::crate_jobs::crate_jobs_tests::group(
        "demo",
        velnor_actions_rust::TaskKind::Clippy,
        &[],
    );
    let (obligations, bindings) =
        crate::crate_jobs::compile_obligations(&[&task], &ToolCatalog::pinned(), "ubuntu-26.04")
            .expect("compiled original proposal");
    let obligation = &obligations[0];
    let Some(ExecutionRecipe::AlreadyReportedCompiler(recipe)) =
        bindings.execution_for(obligation).expect("compiler recipe")
    else {
        panic!("compiler expected");
    };
    assert_eq!(recipe.argv(), obligation.run.as_slice());
    assert_eq!(recipe.task_digest(), obligation.task_digest);
    let mut substituted = obligation.clone();
    substituted.task_digest = format!("b3-{}", "b".repeat(64));
    assert!(bindings.execution_for(&substituted).is_err());
}
