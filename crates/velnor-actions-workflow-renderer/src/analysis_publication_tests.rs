//! Publication expressions remain confined to the exact producer factory.

use super::{upload_step, validate_upload_binding};
use crate::expressions::check_with_value;
use velnor_actions_contract::StepKind;

#[test]
fn output_bindings_accept_only_exact_keys_step_and_field() {
    for (key, value) in [
        ("name", "${{ steps.plan.outputs.analysis_artifact_name }}"),
        ("path", "${{ steps.plan.outputs.analysis_artifact_path }}"),
    ] {
        assert!(check_with_value(key, value).is_ok());
    }
    for (key, value) in [
        ("path", "${{ steps.plan.outputs.analysis_artifact_name }}"),
        ("name", "${{ steps.plan.outputs.analysis_artifact_path }}"),
        ("other", "${{ steps.plan.outputs.analysis_artifact_name }}"),
        ("name", "${{ steps.other.outputs.analysis_artifact_name }}"),
        ("name", "${{ needs.plan.outputs.analysis_artifact_name }}"),
        ("name", "${{ steps.plan.outputs.other }}"),
        (
            "name",
            "prefix-${{ steps.plan.outputs.analysis_artifact_name }}",
        ),
        (
            "name",
            "${{ steps.plan.outputs.analysis_artifact_name }}${{ github.run_id }}",
        ),
    ] {
        assert!(check_with_value(key, value).is_err(), "{key}: {value}");
    }
}

#[test]
fn upload_factory_requires_plan_job_exact_pin_and_protected_condition() {
    let step = upload_step().expect("fixed producer upload");
    assert!(validate_upload_binding("plan", &step).is_ok());
    for job in ["candidate", "publish-baseline", "rust-core", "other"] {
        assert!(validate_upload_binding(job, &step).is_err(), "{job}");
    }
    let mut changed = step.clone();
    changed.condition = Some("always()".to_owned());
    assert!(validate_upload_binding("plan", &changed).is_err());
    let mut changed = step.clone();
    if let StepKind::Action { uses, .. } = &mut changed.kind {
        *uses = format!("actions/download-artifact@{}", "a".repeat(40));
    }
    assert!(validate_upload_binding("plan", &changed).is_err());
    let mut changed = step;
    if let StepKind::Action { with, .. } = &mut changed.kind {
        with.insert(
            "path".to_owned(),
            "${{ steps.plan.outputs.analysis_artifact_name }}".to_owned(),
        );
    }
    assert!(validate_upload_binding("plan", &changed).is_err());
}
