use super::super::*;
use super::admission_fixture::{
    BuiltNode, admission_for, completed_node, phase_plan, useful_delta,
};
use super::directive_tests::qualification_plan;
use super::{SOURCE_A, SOURCE_B, TestResult};
use crate::workflow::{
    QualificationCacheLayer, QualificationDispatchMode, QualificationPhase,
    verification_repeat_campaign,
};

#[test]
fn complete_cold_warm_third_useful_delta_and_control_admission_paths() -> TestResult {
    let cold_plan = qualification_plan(QualificationPhase::Cold, 700, SOURCE_A, None)?;
    let cold = completed_node(&cold_plan, None, None)?;
    assert_eq!(cold.value["producer"]["mode"], "controlled");

    let warm_plan = phase_plan(QualificationPhase::Warm, 701, SOURCE_A, 700)?;
    let cold_admission = admission_for(&cold, None)?;
    cold_admission.validate_for_plan(&warm_plan)?;
    let warm = completed_node(&warm_plan, Some(&cold), None)?;

    let third_plan = phase_plan(QualificationPhase::Third, 702, SOURCE_A, 701)?;
    let warm_admission = admission_for(&warm, None)?;
    warm_admission.validate_for_plan(&third_plan)?;
    let third = completed_node(&third_plan, Some(&warm), None)?;
    assert_warm_successor_is_selected(&cold, &warm, &third, &third_plan, &warm_admission)?;

    let delta = useful_delta(SOURCE_A, SOURCE_B)?;
    let useful_plan = phase_plan(QualificationPhase::UsefulDelta, 703, SOURCE_B, 702)?;
    let third_admission = admission_for(&third, Some(&delta))?;
    third_admission.validate_for_plan(&useful_plan)?;
    let useful = completed_node(&useful_plan, Some(&third), Some(delta.clone()))?;
    assert_eq!(useful.receipt.source_delta, Some(delta));

    let directive = QualificationCacheDirective::for_plan(&useful_plan, Some(&third_admission))?
        .ok_or_else(|| std::io::Error::other("useful-delta directive missing"))?;
    let entry = &useful_plan.matrix.include[0];
    let sources = directive
        .layer(&entry.matrix_key, QualificationCacheLayer::CargoSources)
        .ok_or_else(|| std::io::Error::other("Cargo sources directive missing"))?;
    let keys = directive.bind_runtime_owner(
        &useful_plan,
        Some(&third_admission),
        &entry.job_id,
        QualificationCacheLayer::CargoSources,
        &super::directive_tests::runtime_identity(
            sources
                .runtime
                .as_ref()
                .ok_or_else(|| std::io::Error::other("runtime requirements missing"))?,
        ),
    )?;
    assert!(
        keys.restore
            .as_ref()
            .is_some_and(|restore| restore.expected_cache.is_some())
    );
    assert!(keys.save_if_state_changes);
    assert!(keys.save_key.is_some());

    let control_plan = qualification_plan(QualificationPhase::Control, 704, SOURCE_A, None)?;
    completed_node(&control_plan, None, None)?;
    let control_directive = QualificationCacheDirective::for_plan(&control_plan, None)?
        .ok_or_else(|| std::io::Error::other("control directive missing"))?;
    assert!(
        control_directive
            .owners
            .iter()
            .flat_map(|owner| &owner.layers)
            .all(|layer| !layer.active
                && layer.restore_policy == QualificationCacheRestorePolicy::Disabled
                && layer.save_policy == QualificationCacheSavePolicy::Disabled)
    );
    Ok(())
}

#[test]
fn admission_exposes_its_validated_receipt_and_artifact_link() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 705, SOURCE_A, None)?;
    let cold = completed_node(&plan, None, None)?;
    let admission = admission_for(&cold, None)?;
    let link = admission.receipt_link()?;

    assert_eq!(link.run, cold.receipt.run);
    assert_eq!(link.receipt_digest, admission.receipt_digest()?);
    assert_eq!(link.artifact_id, cold.artifact.id);
    assert_eq!(link.artifact_digest, cold.artifact.digest);
    Ok(())
}

#[test]
fn useful_delta_keeps_portable_owner_cache_identity() -> TestResult {
    let cold_plan = qualification_plan(QualificationPhase::Cold, 760, SOURCE_A, None)?;
    let cold = completed_node(&cold_plan, None, None)?;
    let warm_plan = phase_plan(QualificationPhase::Warm, 761, SOURCE_A, 760)?;
    let warm = completed_node(&warm_plan, Some(&cold), None)?;
    let third_plan = phase_plan(QualificationPhase::Third, 762, SOURCE_A, 761)?;
    let third = completed_node(&third_plan, Some(&warm), None)?;
    let delta = useful_delta(SOURCE_A, SOURCE_B)?;
    let mut useful_plan = phase_plan(QualificationPhase::UsefulDelta, 763, SOURCE_B, 762)?;
    let changed_input = crate::digest_b3(b"useful source edit input");
    useful_plan.matrix.include[0].input_digest = changed_input.clone();
    useful_plan.obligations[0].input_digest = changed_input;
    useful_plan.validate()?;

    let warm_admission = admission_for(&warm, None)?;
    let third_admission = admission_for(&third, Some(&delta))?;
    third_admission.validate_for_plan(&useful_plan)?;
    let useful = completed_node(&useful_plan, Some(&third), Some(delta))?;
    let third_directive =
        QualificationCacheDirective::for_plan(&third_plan, Some(&warm_admission))?
            .ok_or_else(|| std::io::Error::other("third directive missing"))?;
    let directive = QualificationCacheDirective::for_plan(&useful_plan, Some(&third_admission))?
        .ok_or_else(|| std::io::Error::other("useful-delta directive missing"))?;
    let third_owner = &third_directive.owners[0];
    let useful_owner = &directive.owners[0];
    assert_ne!(third_owner.member_digest, useful_owner.member_digest);
    assert_eq!(
        third_owner.layers[1].identity_digest,
        useful_owner.layers[1].identity_digest
    );
    assert_eq!(
        useful.receipt.owners[0].member_digest,
        useful_owner.member_digest
    );
    let runtime = super::super::directive_tests::runtime_identity(
        directive.owners[0].layers[1]
            .runtime
            .as_ref()
            .ok_or_else(|| std::io::Error::other("owner runtime is missing"))?,
    );
    let bound = directive.bind_runtime_owner(
        &useful_plan,
        Some(&third_admission),
        "rust-demo",
        QualificationCacheLayer::CargoSources,
        &runtime,
    )?;
    assert!(
        bound
            .restore
            .is_some_and(|restore| restore.expected_cache.is_some())
    );
    Ok(())
}

#[test]
fn same_source_relocated_checkout_admits_and_binds_prior_owner_cache() -> TestResult {
    let mut cold_plan = qualification_plan(QualificationPhase::Cold, 770, SOURCE_A, None)?;
    relocate_demo_plan(&mut cold_plan, "/home/runner/work/velnor/velnor");
    let cold = completed_node(&cold_plan, None, None)?;
    let mut warm_plan = phase_plan(QualificationPhase::Warm, 771, SOURCE_A, 770)?;
    relocate_demo_plan(&mut warm_plan, "/tmp/agent/_work/velnor/velnor");
    warm_plan.validate()?;

    let admission = admission_for(&cold, None)?;
    admission.validate_for_plan(&warm_plan)?;
    let warm = completed_node(&warm_plan, Some(&cold), None)?;
    let directive = QualificationCacheDirective::for_plan(&warm_plan, Some(&admission))?
        .ok_or_else(|| std::io::Error::other("warm directive missing"))?;
    let layer = directive
        .owner_layer("rust-demo", QualificationCacheLayer::CargoSources)
        .ok_or_else(|| std::io::Error::other("warm owner layer missing"))?;
    assert_eq!(
        layer.identity_digest,
        cold.receipt.owners[0].layers[1].identity_digest
    );
    let runtime = super::super::directive_tests::runtime_identity(
        layer
            .runtime
            .as_ref()
            .ok_or_else(|| std::io::Error::other("warm runtime requirements missing"))?,
    );
    let bound = directive.bind_runtime_owner(
        &warm_plan,
        Some(&admission),
        "rust-demo",
        QualificationCacheLayer::CargoSources,
        &runtime,
    )?;
    assert!(
        bound
            .restore
            .is_some_and(|restore| restore.expected_cache.is_some())
    );
    assert_eq!(
        warm.receipt.owners[0].member_digest,
        cold.receipt.owners[0].member_digest
    );
    Ok(())
}

fn relocate_demo_plan(plan: &mut crate::workflow::Plan, checkout: &str) {
    let package_id = format!("path+file://{checkout}/crates/demo#demo@0.1.0");
    plan.packages[0].package_id = package_id.clone();
    plan.matrix.include[0].adapter_metadata["package_id"] = package_id.clone().into();
    plan.matrix.include[0].adapter_metadata["unit_id"] = package_id.into();
}

#[test]
fn completed_receipt_uses_the_unprotected_verification_branch_identity() -> TestResult {
    let mut plan = qualification_plan(QualificationPhase::Cold, 740, SOURCE_A, None)?;
    let context = plan
        .qualification
        .as_mut()
        .ok_or_else(|| std::io::Error::other("qualification context missing"))?;
    context.git_ref = "refs/heads/sea/integration".to_owned();
    context.ref_protected = false;
    context.workflow_ref = format!(
        "{}/{}@{}",
        context.repository,
        crate::workflow::QUALIFICATION_WORKFLOW_PATH,
        context.git_ref
    );
    context.mode = QualificationDispatchMode::VerificationOnly;
    context.campaign = verification_repeat_campaign(
        &context.repository,
        &context.default_branch,
        &context.git_ref,
        &context.workflow_ref,
        &context.workflow_sha,
        &context.source_sha,
        context.run_id,
    );
    plan.validate()?;

    let node = completed_node(&plan, None, None)?;
    let metadata: crate::workflow::QualificationCacheRunMetadata =
        serde_json::from_value(node.value["metadata"].clone())?;
    assert_eq!(metadata.git_ref, "refs/heads/sea/integration");
    assert!(!metadata.ref_protected);
    assert_eq!(node.artifact.workflow_head_branch, "sea/integration");
    assert_eq!(node.value["producer"]["mode"], "verification_only");

    let mut wrong_mode = node.document.clone();
    wrong_mode.producer.mode = QualificationDispatchMode::Controlled;
    assert!(wrong_mode.to_bounded_json(&plan, None).is_err());

    let mut wrong_ref = metadata.clone();
    wrong_ref.git_ref = "refs/heads/main".to_owned();
    assert!(
        node.document
            .validate_for_plan(&plan, &wrong_ref, None)
            .is_err()
    );

    let mut wrong_protection = metadata;
    wrong_protection.ref_protected = true;
    assert!(
        node.document
            .validate_for_plan(&plan, &wrong_protection, None)
            .is_err()
    );
    Ok(())
}

#[test]
fn producer_document_serializes_pending_only_with_exact_verified_lanes() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 742, SOURCE_A, None)?;
    let node = completed_node(&plan, None, None)?;
    let bytes = node.document.to_bounded_json(&plan, None)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert!(value.get("evidence").is_some());
    assert!(value.get("receipt").is_none());
    assert!(value.get("metadata").is_none());
    assert!(value.get("conclusion").is_none());

    let mut missing_lane = node.document.clone();
    missing_lane.evidence.lanes.clear();
    assert!(missing_lane.to_bounded_json(&plan, None).is_err());

    let mut wrong_lane = node.document.clone();
    wrong_lane.evidence.lanes[0].matrix_key = "unplanned-lane".to_owned();
    assert!(wrong_lane.to_bounded_json(&plan, None).is_err());
    Ok(())
}

fn assert_warm_successor_is_selected(
    cold: &BuiltNode,
    warm: &BuiltNode,
    third: &BuiltNode,
    third_plan: &crate::workflow::Plan,
    warm_admission: &QualificationCacheAdmission,
) -> TestResult {
    let cold_layers = &cold.receipt.owners[0].layers;
    let warm_layers = &warm.receipt.owners[0].layers;
    let third_layers = &third.receipt.owners[0].layers;
    let cold_sources = &cold_layers[1];
    let cold_tools = &cold_layers[2];
    let warm_sources = &warm_layers[1];
    let warm_tools = &warm_layers[2];
    assert_ne!(cold_sources.state_digest, warm_sources.state_digest);
    assert_eq!(
        warm_sources.save.action,
        QualificationCacheSaveActionResult::Succeeded
    );
    assert!(matches!(
        &warm_sources.save.after,
        QualificationCacheBackendObservation::Found(_)
    ));
    assert_eq!(cold_tools.state_digest, warm_tools.state_digest);
    assert_eq!(
        warm_tools.save.action,
        QualificationCacheSaveActionResult::NotRequired
    );

    let directive = QualificationCacheDirective::for_plan(third_plan, Some(warm_admission))?
        .ok_or_else(|| std::io::Error::other("third directive missing"))?;
    let matrix_key = &third_plan.matrix.include[0].matrix_key;
    let sources = directive
        .layer(matrix_key, QualificationCacheLayer::CargoSources)
        .ok_or_else(|| std::io::Error::other("third sources directive missing"))?;
    let tools = directive
        .layer(matrix_key, QualificationCacheLayer::MiseTools)
        .ok_or_else(|| std::io::Error::other("third tools directive missing"))?;
    assert_eq!(
        sources.restore.as_ref().map(|restore| restore.slot),
        Some(QualificationCacheSlot::K2)
    );
    assert_eq!(
        tools.restore.as_ref().map(|restore| restore.slot),
        Some(QualificationCacheSlot::K1)
    );
    assert_eq!(
        third_layers[1].restore.matched_cache,
        warm_sources.save.after
    );
    assert_eq!(third_layers[2].restore.matched_cache, cold_tools.save.after);
    Ok(())
}
