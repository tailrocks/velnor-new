//! Negative frame tests do not construct or substitute fresh source authority.

use super::validate_compiled_frame;
use crate::task_report::task_report_tests::fixture_plan;
use velnor_actions_contract::{ObligationDecision, Plan};

const TASK: &str = "stack/rust/demo/clippy/default";
const TOOLCHAIN: &str = "rust@1.98.0";

fn recipe(profile: &str) -> Vec<String> {
    vec![
        "cargo".to_owned(),
        "clippy".to_owned(),
        "--profile".to_owned(),
        profile.to_owned(),
    ]
}

fn digest(argv: &[String], toolchain: &str) -> String {
    crate::internal::plan_obligation::task_digest(TASK, argv, toolchain, None, None)
        .expect("canonical recipe")
}

fn plan_with_digest(digest: &str) -> Plan {
    let mut plan = fixture_plan();
    for obligation in &mut plan.obligations {
        if obligation.task_id == TASK {
            obligation.task_digest = digest.to_owned();
        }
    }
    for entry in &mut plan.matrix.include {
        if entry.task_id == TASK {
            entry.task_digest = digest.to_owned();
        }
    }
    plan
}

#[test]
fn same_task_id_cannot_authorize_stale_profile_before_coverage() {
    let before = recipe("profile-a");
    let after = recipe("profile-b");
    for covered in [false, true] {
        let mut plan = plan_with_digest(&digest(&after, TOOLCHAIN));
        if covered {
            plan.obligations
                .iter_mut()
                .find(|ob| ob.task_id == TASK)
                .expect("task")
                .decision = ObligationDecision::CoveredByTrustedBaseline;
            plan.matrix.include.retain(|entry| entry.task_id != TASK);
        }
        let error =
            validate_compiled_frame(&plan, TASK, &digest(&before, TOOLCHAIN), &before, TOOLCHAIN)
                .expect_err("stale frame refused before disposition");
        assert!(
            error.to_string().contains("task_digest_mismatch"),
            "{error}"
        );
    }
}

#[test]
fn framed_digest_cannot_be_detached_from_actual_argv_or_toolchain() {
    let original = recipe("profile-a");
    let digest = digest(&original, TOOLCHAIN);
    let plan = plan_with_digest(&digest);
    for (argv, toolchain) in [
        (recipe("profile-b"), TOOLCHAIN),
        (original.clone(), "rust@1.99.0"),
    ] {
        let error = validate_compiled_frame(&plan, TASK, &digest, &argv, toolchain)
            .expect_err("recipe must hash to baked frame");
        assert!(
            error
                .to_string()
                .contains("rust_frame_recipe_digest_mismatch"),
            "{error}"
        );
    }
}

#[test]
fn forged_descriptor_cannot_substitute_for_compiler_recipe() {
    let original = recipe("profile-a");
    let digest = digest(&original, TOOLCHAIN);
    let mut plan = plan_with_digest(&digest);
    plan.matrix
        .include
        .iter_mut()
        .find(|entry| entry.task_id == TASK)
        .expect("entry")
        .adapter_metadata
        .as_object_mut()
        .expect("object metadata")
        .insert(
            "helper_obligation".to_owned(),
            serde_json::json!({"digest": digest}),
        );
    let error = validate_compiled_frame(&plan, TASK, &digest, &original, TOOLCHAIN)
        .expect_err("wire descriptor never supplies compiler authority");
    assert!(
        error.to_string().contains("rust_frame_foreign_descriptor"),
        "{error}"
    );
}

#[test]
fn metadata_requires_object_and_descriptor_absence() {
    let argv = recipe("profile-a");
    let digest = digest(&argv, TOOLCHAIN);
    for metadata in [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!("helper_obligation"),
        serde_json::json!(false),
        serde_json::json!({"helper_obligation": null}),
    ] {
        let mut plan = plan_with_digest(&digest);
        plan.matrix
            .include
            .iter_mut()
            .find(|entry| entry.task_id == TASK)
            .expect("entry")
            .adapter_metadata = metadata.clone();
        let error = validate_compiled_frame(&plan, TASK, &digest, &argv, TOOLCHAIN)
            .expect_err("nonobject metadata and null descriptors refuse");
        let expected = if metadata.is_object() {
            "rust_frame_foreign_descriptor"
        } else {
            "rust_frame_metadata_invalid"
        };
        assert!(error.to_string().contains(expected), "{error}");
    }
    let plan = plan_with_digest(&digest);
    validate_compiled_frame(&plan, TASK, &digest, &argv, TOOLCHAIN)
        .expect("object with absent descriptor binds frame only");
}

#[test]
fn matching_covered_frame_never_admits_execution() {
    let argv = recipe("profile-a");
    let digest = digest(&argv, TOOLCHAIN);
    let mut plan = plan_with_digest(&digest);
    plan.obligations
        .iter_mut()
        .find(|ob| ob.task_id == TASK)
        .expect("task")
        .decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.matrix.include.retain(|entry| entry.task_id != TASK);
    let error = validate_compiled_frame(&plan, TASK, &digest, &argv, TOOLCHAIN)
        .expect_err("baseline coverage does not grant compiler execution");
    assert!(
        error.to_string().contains("rust_frame_execution_covered"),
        "{error}"
    );
}

#[test]
fn execution_inventory_qualification_missing_denies_without_cargo() {
    let argv = recipe("profile-a");
    let digest = digest(&argv, TOOLCHAIN);
    let plan = plan_with_digest(&digest);
    validate_compiled_frame(&plan, TASK, &digest, &argv, TOOLCHAIN).expect("bound frame");
    let (entry, _) = crate::task_report::entry_and_digest(&plan, TASK).expect("entry");
    let probe = crate::inventory::cargo_probe::CargoProbe::begin();
    let error =
        crate::current_semantic_input_proof::CurrentSemanticInputProof::acquire_for_execution(
            &plan, entry,
        )
        .err()
        .expect("source SDK issuer unavailable");
    assert!(matches!(
        error,
        crate::OrchestratorError::NeedsCargo { problem }
            if problem == "current_rust_inventory_source_sdk_qualification_missing"
    ));
    assert_eq!(probe.attempts(), 0);
}
