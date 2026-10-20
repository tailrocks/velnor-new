use super::admission_fixture::{
    admission_for, admission_from_value, completed_node, phase_plan, useful_delta,
};
use super::directive_tests::qualification_plan;
use super::{SOURCE_A, SOURCE_B, TestResult};
use crate::canonical::digest_b3;
use crate::workflow::{QualificationPhase, QualificationRunRef};

#[test]
fn useful_delta_admission_rejects_missing_wrong_and_unexpected_source_changes() -> TestResult {
    let cold_plan = qualification_plan(QualificationPhase::Cold, 710, SOURCE_A, None)?;
    let cold = completed_node(&cold_plan, None, None)?;
    let warm_plan = phase_plan(QualificationPhase::Warm, 711, SOURCE_A, 710)?;
    let warm = completed_node(&warm_plan, Some(&cold), None)?;
    let third_plan = phase_plan(QualificationPhase::Third, 712, SOURCE_A, 711)?;
    let third = completed_node(&third_plan, Some(&warm), None)?;
    let useful_plan = phase_plan(QualificationPhase::UsefulDelta, 713, SOURCE_B, 712)?;

    let missing = admission_for(&third, None)?;
    assert!(missing.validate_for_plan(&useful_plan).is_err());
    let other_source = "cccccccccccccccccccccccccccccccccccccccc";
    let wrong_delta = useful_delta(SOURCE_A, other_source)?;
    let wrong = admission_for(&third, Some(&wrong_delta))?;
    assert!(wrong.validate_for_plan(&useful_plan).is_err());

    let changed_warm = phase_plan(QualificationPhase::Warm, 714, SOURCE_B, 710)?;
    let cold_admission = admission_for(&cold, None)?;
    assert!(cold_admission.validate_for_plan(&changed_warm).is_err());
    Ok(())
}

#[test]
fn plan_predecessor_reference_is_bound_to_the_admitted_attempt() -> TestResult {
    let cold_plan = qualification_plan(QualificationPhase::Cold, 720, SOURCE_A, None)?;
    let cold = completed_node(&cold_plan, None, None)?;
    let warm_plan = phase_plan(QualificationPhase::Warm, 721, SOURCE_A, 720)?;
    let mut changed_ref_plan = warm_plan.clone();
    let context = changed_ref_plan
        .qualification
        .as_mut()
        .ok_or_else(|| std::io::Error::other("qualification context missing"))?;
    context.predecessor = Some(QualificationRunRef {
        run_id: 720,
        run_attempt: 2,
    });
    let admission = admission_for(&cold, None)?;
    assert!(admission.validate_for_plan(&changed_ref_plan).is_err());
    Ok(())
}

#[test]
fn full_admission_rejects_mutated_artifact_link_runtime_and_layer_state() -> TestResult {
    let cold_plan = qualification_plan(QualificationPhase::Cold, 730, SOURCE_A, None)?;
    let cold = completed_node(&cold_plan, None, None)?;
    let warm_plan = phase_plan(QualificationPhase::Warm, 731, SOURCE_A, 730)?;
    let warm = completed_node(&warm_plan, Some(&cold), None)?;
    let third_plan = phase_plan(QualificationPhase::Third, 732, SOURCE_A, 731)?;
    let third = completed_node(&third_plan, Some(&warm), None)?;
    let useful_plan = phase_plan(QualificationPhase::UsefulDelta, 733, SOURCE_B, 732)?;
    let delta = useful_delta(SOURCE_A, SOURCE_B)?;
    admission_for(&third, Some(&delta))?.validate_for_plan(&useful_plan)?;

    let mut wrong_link = third.value.clone();
    wrong_link["receipt"]["predecessor"]["artifact_digest"] =
        serde_json::json!(format!("sha256:{}", "f".repeat(64)));
    assert!(rejects_admission(&wrong_link, &useful_plan, Some(&delta)));

    let mut wrong_runtime = third.value.clone();
    wrong_runtime["receipt"]["lanes"][0]["layers"][1]["runtime_identity"]["platform"]["image_identity"]
        ["image_os"] = serde_json::json!("ubuntu27");
    assert!(rejects_admission(
        &wrong_runtime,
        &useful_plan,
        Some(&delta)
    ));

    let mut wrong_state = third.value.clone();
    wrong_state["receipt"]["lanes"][0]["layers"][1]["state_digest"] =
        serde_json::json!(digest_b3(b"forged third cargo state"));
    assert!(rejects_admission(&wrong_state, &useful_plan, Some(&delta)));

    let mut wrong_plan_id = third.value.clone();
    wrong_plan_id["receipt"]["plan_id"] = serde_json::json!("plan-p732-a2");
    assert!(rejects_admission(
        &wrong_plan_id,
        &useful_plan,
        Some(&delta)
    ));
    Ok(())
}

pub(super) fn rejects_admission(
    value: &serde_json::Value,
    plan: &crate::workflow::Plan,
    source_delta: Option<&crate::workflow::QualificationSourceDelta>,
) -> bool {
    match admission_from_value(value, source_delta) {
        Ok(admission) => admission.validate_for_plan(plan).is_err(),
        Err(_) => true,
    }
}
