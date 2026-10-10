use std::error::Error;

#[path = "qualification_cache_lineage_admission_fixture.rs"]
mod admission_fixture;

use self::admission_fixture::{
    BuiltNode, admission_for, admission_from_value, completed_node, phase_plan, useful_delta,
};
use super::directive_tests::qualification_plan;
use super::*;
use crate::canonical::digest_b3;
use crate::workflow::{QualificationCacheLayer, QualificationPhase, QualificationRunRef};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const SOURCE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn complete_cold_warm_third_useful_delta_and_control_admission_paths() -> TestResult {
    let cold_plan = qualification_plan(QualificationPhase::Cold, 700, SOURCE_A, None)?;
    let cold = completed_node(&cold_plan, None, None)?;

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
    let keys = directive.bind_runtime(
        &useful_plan,
        Some(&third_admission),
        &entry.matrix_key,
        QualificationCacheLayer::CargoSources,
        &super::directive_tests::runtime_identity(&sources.runtime),
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
            .lanes
            .iter()
            .flat_map(|lane| &lane.layers)
            .all(|layer| !layer.active
                && layer.restore_policy == QualificationCacheRestorePolicy::Disabled
                && layer.save_policy == QualificationCacheSavePolicy::Disabled)
    );
    Ok(())
}

#[test]
fn native_mbx_receipt_and_directive_use_the_object_layer_only() -> TestResult {
    let mut plan = qualification_plan(QualificationPhase::Cold, 705, SOURCE_A, None)?;
    plan.matrix.include[0].adapter_metadata["compile_driver"] = serde_json::json!("mbx");
    plan.validate()?;
    let node = completed_node(&plan, None, None)?;
    let layers = &node.receipt.lanes[0].layers;
    let objects = layers
        .iter()
        .find(|layer| layer.layer == QualificationCacheLayer::MbxObjects)
        .ok_or_else(|| std::io::Error::other("native MBX object receipt missing"))?;
    assert_eq!(layers.len(), 5);
    assert!(objects.active && objects.state_digest.is_some());
    assert!(objects.runtime_identity.is_some());
    assert_eq!(node.document.schema, 2);
    assert_eq!(node.receipt.schema, 2);

    let entry = &plan.matrix.include[0];
    let directive = QualificationCacheDirective::for_plan(&plan, None)?
        .ok_or_else(|| std::io::Error::other("native MBX directive missing"))?;
    let serialized = serde_json::to_value(&directive)?;
    let emitted_layers = serialized["lanes"][0]["layers"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("directive layers are not an array"))?;
    assert_eq!(directive.schema, 2);
    assert_eq!(emitted_layers.len(), 5);
    assert!(
        emitted_layers
            .iter()
            .any(|layer| layer["layer"] == "mbx_objects")
    );
    assert!(!serialized.to_string().contains("mbx_bundle"));
    let native = directive
        .layer(&entry.matrix_key, QualificationCacheLayer::MbxObjects)
        .ok_or_else(|| std::io::Error::other("native MBX directive layer missing"))?;
    let keys = directive.bind_runtime(
        &plan,
        None,
        &entry.matrix_key,
        QualificationCacheLayer::MbxObjects,
        &super::directive_tests::runtime_identity(&native.runtime),
    )?;
    assert!(keys.restore.is_some() && keys.save_key.is_some());

    let mut legacy = serialized;
    legacy["schema"] = serde_json::json!(1);
    let legacy: QualificationCacheDirective = serde_json::from_value(legacy)?;
    assert!(
        legacy
            .bind_runtime(
                &plan,
                None,
                &entry.matrix_key,
                QualificationCacheLayer::MbxObjects,
                &super::directive_tests::runtime_identity(&native.runtime),
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn legacy_receipt_versions_and_artifact_identity_are_rejected() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 706, SOURCE_A, None)?;
    let node = completed_node(&plan, None, None)?;
    let mut old_receipt = node.value.clone();
    old_receipt["receipt"]["schema"] = serde_json::json!(1);
    assert!(rejects_admission(&old_receipt, &plan, None));

    let mut old_artifact = node.value.clone();
    old_artifact["artifact"]["name"] = serde_json::json!("velnor-qualification-cache-receipt-v1");
    assert!(rejects_admission(&old_artifact, &plan, None));

    let metadata: crate::workflow::QualificationCacheRunMetadata =
        serde_json::from_value(node.value["metadata"].clone())?;
    let mut old_document = node.document.clone();
    old_document.schema = 1;
    assert!(
        old_document
            .to_bounded_json(&plan, &metadata, None)
            .is_err()
    );
    Ok(())
}

#[test]
fn receipt_admission_rejects_missing_extra_and_retired_layers() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 707, SOURCE_A, None)?;
    let node = completed_node(&plan, None, None)?;

    let mut active_mbx_objects = node.value.clone();
    let mbx_objects = active_mbx_objects["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?
        .iter_mut()
        .find(|layer| layer["layer"] == "mbx_objects")
        .ok_or_else(|| std::io::Error::other("MBX objects receipt is missing"))?;
    assert_eq!(mbx_objects["active"], false);
    mbx_objects["active"] = serde_json::json!(true);
    assert!(rejects_admission(&active_mbx_objects, &plan, None));

    let mut missing = node.value.clone();
    missing["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?
        .pop();
    assert!(rejects_admission(&missing, &plan, None));

    let mut extra = node.value.clone();
    let extra_layer = extra["receipt"]["lanes"][0]["layers"][0].clone();
    extra["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?
        .push(extra_layer);
    assert!(rejects_admission(&extra, &plan, None));

    let mut duplicate = node.value.clone();
    let layers = duplicate["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?;
    let first_layer = layers[0].clone();
    layers[1] = first_layer;
    assert_eq!(layers.len(), 5);
    assert!(rejects_admission(&duplicate, &plan, None));

    let mut reordered = node.value.clone();
    let layers = reordered["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?;
    layers.swap(0, 1);
    assert_eq!(layers.len(), 5);
    assert!(rejects_admission(&reordered, &plan, None));

    let mut retired = node.value.clone();
    retired["receipt"]["lanes"][0]["layers"][0]["layer"] = serde_json::json!("mbx_bundle");
    assert!(rejects_admission(&retired, &plan, None));
    Ok(())
}

fn assert_warm_successor_is_selected(
    cold: &BuiltNode,
    warm: &BuiltNode,
    third: &BuiltNode,
    third_plan: &crate::workflow::Plan,
    warm_admission: &QualificationCacheAdmission,
) -> TestResult {
    let cold_layers = &cold.receipt.lanes[0].layers;
    let warm_layers = &warm.receipt.lanes[0].layers;
    let third_layers = &third.receipt.lanes[0].layers;
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
    wrong_runtime["receipt"]["lanes"][0]["layers"][1]["runtime_identity"]["platform"]["image_version"] =
        serde_json::json!("27.04");
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

fn rejects_admission(
    value: &serde_json::Value,
    plan: &crate::workflow::Plan,
    source_delta: Option<&crate::workflow::QualificationSourceDelta>,
) -> bool {
    match admission_from_value(value, source_delta) {
        Ok(admission) => admission.validate_for_plan(plan).is_err(),
        Err(_) => true,
    }
}
