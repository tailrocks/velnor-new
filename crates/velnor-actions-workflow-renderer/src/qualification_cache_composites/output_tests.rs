use super::{after_outputs, before_outputs, cache_id, output_map, outputs, split_tail};
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage, StepId,
    StepRole,
};
use velnor_actions_contract::{Step, StepKind};

#[test]
fn restore_action_fields_cross_the_prepare_boundary_for_the_same_layer() {
    for layer in [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ] {
        let all = outputs("rust-demo").expect("prepare outputs");
        assert!(all.iter().all(|output| {
            !output.name.contains("_restore_census_")
                || output.name.ends_with("_restore_census_action_hit")
                || output.name.ends_with("_restore_census_action_matched_key")
        }));
        let action_id = cache_id(QualificationCacheStepStage::Import, layer, "rust-demo")
            .expect("import identity");
        for (name, field) in [
            ("action_hit", "restore_action_hit"),
            ("action_matched_key", "restore_action_matched_key"),
        ] {
            let name = format!("cache_{}_restore_census_{name}", layer.as_str());
            let output = all
                .iter()
                .find(|output| output.name == name)
                .expect("output");
            assert_eq!(output.id, action_id);
            assert_eq!(output.field, field);
            let mapped = output_map(std::slice::from_ref(output), "prepare_outputs");
            assert_eq!(
                mapped.get(&format!("steps.{}.outputs.{field}", action_id.as_string())),
                Some(&format!("fromJSON(inputs.prepare_outputs).{name}"))
            );
        }
    }
}

#[test]
fn worker_tail_accepts_three_typed_steps_per_layer_and_aggregate() {
    let steps = tail_steps("rust-demo");
    let (layers, aggregate) = split_tail(steps, "rust-demo").expect("three-step owner tail");
    assert_eq!(layers.len(), 5);
    assert!(aggregate.is_some());
    for chunk in layers {
        assert_eq!(chunk.before.len(), 1);
        assert_eq!(chunk.save.role, Some(StepRole::QualificationCacheSave));
        assert_eq!(chunk.after.len(), 1);
        assert_eq!(
            chunk.after[0].role,
            Some(StepRole::QualificationCacheOwnerLayerEvidence)
        );
        assert_eq!(chunk.record_id.layer(), chunk.layer);
    }
}

#[test]
fn worker_tail_rejects_cross_layer_order_and_legacy_census_steps() {
    let mut steps = tail_steps("rust-demo");
    steps.swap(0, 1);
    assert!(split_tail(steps, "rust-demo").is_err());

    let mut steps = tail_steps("rust-demo");
    let StepId::QualificationCache(save_id) = steps[1].id.expect("save id") else {
        panic!("The save step must have a qualification-cache ID.");
    };
    steps[1].id = Some(StepId::QualificationCache(
        QualificationCacheStepId::new(
            QualificationCacheStepStage::Save,
            QualificationCacheLayer::CargoSources,
            "rust-demo",
        )
        .expect("other layer ID"),
    ));
    assert_ne!(save_id.layer(), QualificationCacheLayer::CargoSources);
    assert!(split_tail(steps, "rust-demo").is_err());

    let mut steps = tail_steps("rust-demo");
    steps.insert(
        1,
        test_step(
            QualificationCacheLayer::MbxObjects,
            QualificationCacheStepStage::SaveBeforeCensus,
            StepRole::QualificationCacheCensus,
            "rust-demo",
        ),
    );
    assert!(split_tail(steps, "rust-demo").is_err());

    assert!(split_tail(tail_steps("other-job"), "rust-demo").is_err());
}

#[test]
fn before_and_after_outputs_match_worker_owned_three_step_contract() {
    let layer = QualificationCacheLayer::MbxObjects;
    let export = test_step(
        layer,
        QualificationCacheStepStage::Export,
        StepRole::QualificationCachePayloadExport,
        "rust-demo",
    );
    let outputs = before_outputs(layer, "rust-demo", &[export]).expect("payload export outputs");
    assert_eq!(outputs.len(), 3);
    assert_eq!(
        outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        [
            "cache_mbx_objects_export_result",
            "cache_mbx_objects_save_requested",
            "cache_mbx_objects_state_digest",
        ]
    );

    let record = test_step(
        layer,
        QualificationCacheStepStage::OwnerLayerEvidence,
        StepRole::QualificationCacheOwnerLayerEvidence,
        "rust-demo",
    );
    assert_eq!(
        after_outputs(layer, "rust-demo", &[record])
            .expect("owner evidence")
            .len(),
        1
    );
}

fn tail_steps(owner: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for layer in [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ] {
        steps.push(test_step(
            layer,
            QualificationCacheStepStage::Export,
            StepRole::QualificationCachePayloadExport,
            owner,
        ));
        steps.push(test_step(
            layer,
            QualificationCacheStepStage::Save,
            StepRole::QualificationCacheSave,
            owner,
        ));
        steps.push(test_step(
            layer,
            QualificationCacheStepStage::OwnerLayerEvidence,
            StepRole::QualificationCacheOwnerLayerEvidence,
            owner,
        ));
    }
    steps.push(test_step(
        QualificationCacheLayer::TaskResult,
        QualificationCacheStepStage::OwnerAggregate,
        StepRole::QualificationCacheOwnerAggregate,
        owner,
    ));
    steps
}

fn test_step(
    layer: QualificationCacheLayer,
    stage: QualificationCacheStepStage,
    role: StepRole,
    owner: &str,
) -> Step {
    Step {
        name: "cache test step".to_owned(),
        id: Some(StepId::QualificationCache(
            QualificationCacheStepId::new(stage, layer, owner).expect("typed step ID"),
        )),
        role: Some(role),
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "cache_test".to_owned(),
            env: std::collections::BTreeMap::new(),
        },
    }
}
