use super::super::*;
use super::admission_fixture::completed_node;
use super::directive_tests::qualification_plan;
use super::rejections::rejects_admission;
use super::{SOURCE_A, TestResult};
use crate::workflow::{QualificationCacheLayer, QualificationPhase};

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
    assert_eq!(node.document.schema, 3);
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
        &super::directive_tests::runtime_identity(
            native
                .runtime
                .as_ref()
                .ok_or_else(|| std::io::Error::other("runtime requirements missing"))?,
        ),
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
                &super::directive_tests::runtime_identity(
                    native
                        .runtime
                        .as_ref()
                        .ok_or_else(|| std::io::Error::other("runtime requirements missing"))?,
                ),
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
    old_artifact["artifact"]["name"] =
        serde_json::json!("velnor-qualification-cache-receipt-v2-a1");
    assert!(rejects_admission(&old_artifact, &plan, None));

    let mut old_document = node.document.clone();
    old_document.schema = 2;
    assert!(old_document.to_bounded_json(&plan, None).is_err());
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

#[test]
fn receipt_admission_requires_a_valid_backend_version() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 708, SOURCE_A, None)?;
    let node = completed_node(&plan, None, None)?;
    let mut malformed = node.value.clone();
    let saved = malformed["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?
        .iter_mut()
        .find(|layer| layer["save"]["after"]["status"] == "found")
        .ok_or_else(|| std::io::Error::other("saved cache entry is missing"))?;
    saved["save"]["after"]["entry"]["version"] = serde_json::json!("bad");
    assert!(rejects_admission(&malformed, &plan, None));

    let mut missing = node.value.clone();
    let saved = missing["receipt"]["lanes"][0]["layers"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("receipt layers are not an array"))?
        .iter_mut()
        .find(|layer| layer["save"]["after"]["status"] == "found")
        .ok_or_else(|| std::io::Error::other("saved cache entry is missing"))?;
    saved["save"]["after"]["entry"]
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("cache entry is not an object"))?
        .remove("version");
    assert!(rejects_admission(&missing, &plan, None));
    Ok(())
}
