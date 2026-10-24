use velnor_actions_contract::workflow::step_identity::validate_step_sequence;
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage, StepId,
};
use velnor_actions_contract::{StepKind, StepRole};

use super::owner_binding_steps;

#[test]
fn owner_binding_prelude_has_unique_typed_ids_and_bound_file_paths() {
    let owner = "rust-example";
    let steps = owner_binding_steps(owner).expect("owner steps");
    assert_eq!(steps.len(), 29);
    assert_all_internal_steps_have_runtime_phase(&steps);
    assert_worker_steps_stay_local_and_token_free(&steps);
    validate_step_sequence(&steps, owner).expect("valid steps");
    let post_import = steps
        .iter()
        .filter(|step| step.role == Some(StepRole::QualificationCacheRuntimeIdentityAfterImport))
        .collect::<Vec<_>>();
    assert_eq!(post_import.len(), 4);
    for layer in [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
    ] {
        let id = QualificationCacheStepId::new(
            QualificationCacheStepStage::RuntimeIdentityAfterImport,
            layer,
            owner,
        )
        .expect("post-import identity id");
        assert!(post_import.iter().any(|step| {
            step.id == Some(StepId::QualificationCache(id))
                && step
                    .condition
                    .as_deref()
                    .is_some_and(|condition| condition.contains("outputs.active == 'true'"))
        }));
    }

    let (chunks, remainder) = steps[..15].as_chunks::<3>();
    assert_eq!(remainder, []);
    for (chunk, layer) in chunks.iter().zip([
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]) {
        let runtime_id = QualificationCacheStepId::new(
            QualificationCacheStepStage::RuntimeIdentity,
            layer,
            owner,
        )
        .expect("runtime id");
        let roots_id =
            QualificationCacheStepId::new(QualificationCacheStepStage::Roots, layer, owner)
                .expect("roots id");
        let bind_id =
            QualificationCacheStepId::new(QualificationCacheStepStage::Bind, layer, owner)
                .expect("bind id");
        assert_eq!(chunk[0].id, Some(StepId::QualificationCache(runtime_id)));
        assert_eq!(chunk[0].role, Some(StepRole::QualificationRuntimeIdentity));
        assert_eq!(chunk[1].id, Some(StepId::QualificationCache(roots_id)));
        assert_eq!(chunk[1].role, Some(StepRole::QualificationCacheRoots));
        assert_eq!(chunk[2].id, Some(StepId::QualificationCache(bind_id)));
        assert_eq!(chunk[2].role, Some(StepRole::QualificationCacheBind));
        assert_runtime_phase(
            &chunk[2],
            crate::QualificationCacheRuntimePhase::BeforeImport,
        );
        assert!(chunk.iter().all(|step| {
            step.condition.as_deref() == Some("needs.plan.outputs.qualification_campaign != ''")
        }));
        assert_bound_file_paths(&chunk[2], runtime_id, roots_id);
        assert_runtime_phase(
            &chunk[0],
            crate::QualificationCacheRuntimePhase::BeforeImport,
        );
    }
}

fn assert_bound_file_paths(
    step: &velnor_actions_contract::Step,
    runtime_id: QualificationCacheStepId,
    roots_id: QualificationCacheStepId,
) {
    let StepKind::Internal { env, .. } = &step.kind else {
        panic!("bind is internal");
    };
    for (key, value) in [
        (
            "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE",
            format!(
                "${{{{ steps.{}.outputs.runtime_identity_path }}}}",
                runtime_id.as_string()
            ),
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE",
            format!(
                "${{{{ steps.{}.outputs.root_observation_path }}}}",
                roots_id.as_string()
            ),
        ),
    ] {
        assert_eq!(env.get(key), Some(&value));
    }
}

fn assert_runtime_phase(
    step: &velnor_actions_contract::Step,
    expected: crate::QualificationCacheRuntimePhase,
) {
    let StepKind::Internal { env, .. } = &step.kind else {
        panic!("runtime identity is internal");
    };
    let generated_phase = env
        .get("VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE")
        .expect("runtime phase from generated step environment");
    assert_eq!(
        crate::QualificationCacheRuntimePhase::parse(generated_phase)
            .expect("same parser used by CLI dispatch"),
        expected
    );
}

#[test]
fn owner_evidence_runs_after_parallel_tasks_and_has_one_aggregate() {
    let owner = "rust-example";
    let steps = super::owner_evidence_steps(owner).expect("owner evidence steps");
    assert_eq!(steps.len(), 16);
    validate_step_sequence(&steps, owner).expect("valid evidence sequence");
    assert_all_internal_steps_have_runtime_phase(&steps);
    assert_worker_steps_stay_local_and_token_free(&steps);
    assert_eq!(
        steps.last().expect("aggregate").role,
        Some(StepRole::QualificationCacheOwnerAggregate)
    );
    let recorders = steps
        .iter()
        .filter(|step| step.role == Some(StepRole::QualificationCacheOwnerLayerEvidence))
        .count();
    assert_eq!(recorders, 5);
    assert_owner_evidence_layers(&steps, owner);
    assert!(
        steps
            .iter()
            .filter(|step| step.role == Some(StepRole::QualificationCacheSave))
            .all(|step| {
                step.condition.as_deref().is_some_and(|condition| {
                    condition.contains("outputs.save_key != ''")
                        && condition.contains("outputs.save_requested == 'true'")
                })
            })
    );
    let exports = steps
        .iter()
        .filter(|step| step.role == Some(StepRole::QualificationCachePayloadExport));
    assert!(exports.clone().all(|step| {
        step.condition
            .as_deref()
            .is_some_and(|condition| condition.contains("outputs.active == 'true'"))
    }));
    for step in exports {
        assert_export_phase(step);
    }
}

fn assert_all_internal_steps_have_runtime_phase(steps: &[velnor_actions_contract::Step]) {
    for step in steps {
        let StepKind::Internal { env, .. } = &step.kind else {
            continue;
        };
        let phase = env
            .get("VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE")
            .expect("cache operation runtime phase");
        crate::QualificationCacheRuntimePhase::parse(phase)
            .expect("same phase parser used by CLI dispatch");
    }
}

fn assert_owner_evidence_layers(steps: &[velnor_actions_contract::Step], owner: &str) {
    let tail = &steps[..15];
    let (groups, remainder) = tail.as_chunks::<3>();
    assert_eq!(remainder, []);
    for (group, layer) in groups.iter().zip(cache_layers()) {
        for (index, (stage, role)) in [
            (
                QualificationCacheStepStage::Export,
                StepRole::QualificationCachePayloadExport,
            ),
            (
                QualificationCacheStepStage::Save,
                StepRole::QualificationCacheSave,
            ),
            (
                QualificationCacheStepStage::OwnerLayerEvidence,
                StepRole::QualificationCacheOwnerLayerEvidence,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let id = QualificationCacheStepId::new(stage, layer, owner)
                .expect("typed owner layer identity");
            assert_eq!(group[index].id, Some(StepId::QualificationCache(id)));
            assert_eq!(group[index].role, Some(role));
        }
        assert_eq!(
            group[2].condition.as_deref(),
            Some("needs.plan.outputs.qualification_campaign != ''"),
            "the evidence record runs when the campaign is active, even when the save is skipped"
        );
        let StepKind::Internal { env, .. } = &group[2].kind else {
            panic!("owner layer evidence is internal");
        };
        assert_eq!(
            env.get("VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE")
                .map(String::as_str),
            Some("before_import")
        );
        let save_action = env
            .get("VELNOR_QUALIFICATION_CACHE_SAVE_ACTION")
            .expect("save action output");
        assert!(save_action.contains("outputs.save_requested == 'true'"));
        assert!(save_action.contains("|| ''"));
    }
    assert!(steps.iter().all(|step| {
        step.role != Some(StepRole::QualificationCacheCensus)
            && !step.id.is_some_and(|id| match id {
                StepId::QualificationCache(id) => matches!(
                    id.stage(),
                    QualificationCacheStepStage::SaveBeforeCensus
                        | QualificationCacheStepStage::SaveAfterCensus
                ),
                _ => false,
            })
    }));
    let aggregate = steps.last().expect("aggregate");
    let aggregate_id = QualificationCacheStepId::new(
        QualificationCacheStepStage::OwnerAggregate,
        QualificationCacheLayer::TaskResult,
        owner,
    )
    .expect("aggregate identity");
    assert_eq!(aggregate.id, Some(StepId::QualificationCache(aggregate_id)));
}

fn cache_layers() -> [QualificationCacheLayer; 5] {
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
}

fn assert_worker_steps_stay_local_and_token_free(steps: &[velnor_actions_contract::Step]) {
    for step in steps {
        let env = match &step.kind {
            StepKind::Action { uses, env, with } => {
                assert!(uses.starts_with("actions/cache/"));
                assert!(!with.contains_key("GH_TOKEN"));
                assert!(!with.contains_key("GITHUB_TOKEN"));
                env
            }
            StepKind::Internal { operation, env } => {
                assert!(matches!(
                    operation.as_str(),
                    "observe-qualification-cache-runtime-v1"
                        | "observe-qualification-cache-roots-v1"
                        | "bind-qualification-cache-v1"
                        | "import-qualification-cache-payload-v1"
                        | "export-qualification-cache-payload-v1"
                        | "record-qualification-cache-owner-layer-v1"
                        | "aggregate-qualification-cache-owner-v1"
                ));
                env
            }
            StepKind::Shell { .. } | StepKind::TaskExecution { .. } => {
                panic!("cache ownership operations must not launch worker REST commands");
            }
        };
        assert!(!env.contains_key("GH_TOKEN"));
        assert!(!env.contains_key("GITHUB_TOKEN"));
    }
}

fn assert_export_phase(step: &velnor_actions_contract::Step) {
    let StepKind::Internal { env, .. } = &step.kind else {
        panic!("export is internal");
    };
    let phase = env
        .get("VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE")
        .expect("generated export phase");
    let parsed = crate::QualificationCacheRuntimePhase::parse(phase)
        .expect("same phase parser used by the CLI");
    let layer = env
        .get("VELNOR_QUALIFICATION_CACHE_LAYER")
        .expect("generated layer");
    let expected = if layer == "task_result" {
        crate::QualificationCacheRuntimePhase::BeforeImport
    } else {
        crate::QualificationCacheRuntimePhase::AfterImport
    };
    assert_eq!(parsed, expected);
}

#[test]
fn owner_binding_prelude_rejects_invalid_owner_ids() {
    assert!(owner_binding_steps("Rust Example").is_err());
}
