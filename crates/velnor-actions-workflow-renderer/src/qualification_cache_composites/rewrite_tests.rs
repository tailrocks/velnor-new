use super::super::output::output_map;
use super::*;
use velnor_actions_contract::{Step, StepKind};

fn owner_record_step(
    layer: QualificationCacheLayer,
    owner_id: &str,
    restore_layer: QualificationCacheLayer,
    restore_owner_id: &str,
) -> Step {
    let record = cache_id(
        QualificationCacheStepStage::OwnerLayerEvidence,
        layer,
        owner_id,
    )
    .expect("record ID");
    let restore = cache_id(
        QualificationCacheStepStage::Import,
        restore_layer,
        restore_owner_id,
    )
    .expect("import ID");
    Step {
        name: "Record cache facts".to_owned(),
        id: Some(StepId::QualificationCache(record)),
        role: Some(StepRole::QualificationCacheOwnerLayerEvidence),
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "record-qualification-cache-owner-layer-v1".to_owned(),
            env: BTreeMap::from([
                (
                    "VELNOR_QUALIFICATION_CACHE_HIT".to_owned(),
                    format!(
                        "${{{{ steps.{}.outputs.restore_action_hit }}}}",
                        restore.as_string()
                    ),
                ),
                (
                    "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY".to_owned(),
                    format!(
                        "${{{{ steps.{}.outputs.restore_action_matched_key }}}}",
                        restore.as_string()
                    ),
                ),
            ]),
        },
    }
}

#[test]
fn restore_action_facts_rewrite_only_from_the_same_owner_layer() {
    let outputs = output_map(
        &super::super::output::outputs("rust-demo").expect("outputs"),
        "prepare_outputs",
    );
    let step = owner_record_step(
        QualificationCacheLayer::CargoSources,
        "rust-demo",
        QualificationCacheLayer::CargoSources,
        "rust-demo",
    );
    let rewritten = rewrite_cross_phase_outputs(vec![step], &outputs).expect("rewrite");
    let StepKind::Internal { env, .. } = &rewritten[0].kind else {
        panic!("record step must remain internal");
    };
    assert_eq!(
            env.get("VELNOR_QUALIFICATION_CACHE_HIT"),
            Some(&"${{ fromJSON(inputs.prepare_outputs).cache_cargo_sources_restore_census_action_hit }}".to_owned())
        );
    assert_eq!(
            env.get("VELNOR_QUALIFICATION_CACHE_MATCHED_KEY"),
            Some(&"${{ fromJSON(inputs.prepare_outputs).cache_cargo_sources_restore_census_action_matched_key }}".to_owned())
        );
}

#[test]
fn restore_action_facts_reject_foreign_owner_and_layer_steps() {
    let outputs = output_map(
        &super::super::output::outputs("rust-demo").expect("outputs"),
        "prepare_outputs",
    );
    for (restore_layer, restore_owner) in [
        (QualificationCacheLayer::CargoSources, "foreign-job"),
        (QualificationCacheLayer::MbxObjects, "rust-demo"),
    ] {
        assert!(
            rewrite_cross_phase_outputs(
                vec![owner_record_step(
                    QualificationCacheLayer::CargoSources,
                    "rust-demo",
                    restore_layer,
                    restore_owner,
                )],
                &outputs
            )
            .is_err()
        );
    }
}

#[test]
fn after_save_inputs_bind_prepare_before_and_save_to_one_owner() {
    let layer = QualificationCacheLayer::CargoSources;
    let prepare =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, "rust-demo")
            .expect("prepare ID");
    let before = QualificationCacheCompositeId::for_layer(
        QualificationCacheCompositePhase::BeforeSave,
        layer,
        "rust-demo",
    )
    .expect("before-save ID");
    let save = cache_id(QualificationCacheStepStage::Save, layer, "rust-demo").expect("save ID");
    let outcome = format!("${{{{ steps.{}.outcome }}}}", save.as_string());
    let inputs = inputs_for_after(layer, prepare, before, save, "rust-demo", &outcome)
        .expect("after-save inputs");
    assert_eq!(
        inputs.get("prepare_outputs"),
        Some(&format!(
            "${{{{ toJSON(steps.{}.outputs) }}}}",
            prepare.as_string()
        ))
    );
    assert_eq!(
        inputs.get("before_outputs"),
        Some(&format!(
            "${{{{ toJSON(steps.{}.outputs) }}}}",
            before.as_string()
        ))
    );
    assert_eq!(
        inputs.get("cache_cargo_sources_save_action_outcome"),
        Some(&outcome)
    );
}

#[test]
fn after_save_inputs_reject_foreign_prepare_or_before_owner() {
    let layer = QualificationCacheLayer::CargoSources;
    let prepare = QualificationCacheCompositeId::new(
        QualificationCacheCompositePhase::Prepare,
        "foreign-job",
    )
    .expect("foreign prepare ID");
    let before = QualificationCacheCompositeId::for_layer(
        QualificationCacheCompositePhase::BeforeSave,
        layer,
        "rust-demo",
    )
    .expect("before-save ID");
    let save = cache_id(QualificationCacheStepStage::Save, layer, "rust-demo").expect("save ID");
    assert!(inputs_for_after(layer, prepare, before, save, "rust-demo", "").is_err());

    let prepare =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, "rust-demo")
            .expect("prepare ID");
    let foreign_before = QualificationCacheCompositeId::for_layer(
        QualificationCacheCompositePhase::BeforeSave,
        layer,
        "foreign-job",
    )
    .expect("foreign before-save ID");
    assert!(inputs_for_after(layer, prepare, foreign_before, save, "rust-demo", "").is_err());
}
