use super::{
    LAYERS, ROOTS_FILE_ENV, RUNTIME_FILE_ENV, cache_action_step, dynamic_id, internal_dynamic_step,
    owner_layer_env, step_output,
};
use crate::{OrchestratorError, QualificationCacheRuntimePhase};
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage,
};
use velnor_actions_contract::{Step, StepRole};

const AFTER_RUNTIME_FILE_ENV: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_AFTER_FILE";

struct LayerIds {
    bind: QualificationCacheStepId,
    export: QualificationCacheStepId,
    save: QualificationCacheStepId,
    record: QualificationCacheStepId,
}

/// Export, save, and record each owner layer after the parallel task group.
pub(in crate::crate_jobs) fn owner_evidence_steps(
    owner_id: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(LAYERS.len() * 3 + 1);
    let mut evidence = Vec::with_capacity(LAYERS.len());
    for layer in LAYERS {
        append_layer_evidence(owner_id, layer, &mut steps, &mut evidence)?;
    }
    steps.push(owner_aggregate(owner_id, evidence)?);
    Ok(steps)
}

fn append_layer_evidence(
    owner_id: &str,
    layer: QualificationCacheLayer,
    steps: &mut Vec<Step>,
    evidence: &mut Vec<(QualificationCacheLayer, QualificationCacheStepId)>,
) -> Result<(), OrchestratorError> {
    let ids = layer_ids(owner_id, layer)?;
    steps.push(export_step(owner_id, layer, &ids)?);
    steps.push(save_action(layer, &ids)?);
    steps.push(layer_record(owner_id, layer, &ids)?);
    evidence.push((layer, ids.record));
    Ok(())
}

fn layer_ids(
    owner_id: &str,
    layer: QualificationCacheLayer,
) -> Result<LayerIds, OrchestratorError> {
    Ok(LayerIds {
        bind: dynamic_id(QualificationCacheStepStage::Bind, layer, owner_id)?,
        export: dynamic_id(QualificationCacheStepStage::Export, layer, owner_id)?,
        save: dynamic_id(QualificationCacheStepStage::Save, layer, owner_id)?,
        record: dynamic_id(
            QualificationCacheStepStage::OwnerLayerEvidence,
            layer,
            owner_id,
        )?,
    })
}

fn export_step(
    owner_id: &str,
    layer: QualificationCacheLayer,
    ids: &LayerIds,
) -> Result<Step, OrchestratorError> {
    let mut env = owner_layer_env(owner_id, layer);
    let runtime_stage = if layer == QualificationCacheLayer::TaskResult {
        QualificationCacheStepStage::RuntimeIdentity
    } else {
        QualificationCacheStepStage::RuntimeIdentityAfterImport
    };
    env.insert(
        RUNTIME_FILE_ENV.to_owned(),
        step_output(
            dynamic_id(runtime_stage, layer, owner_id)?,
            "runtime_identity_path",
        ),
    );
    env.insert(
        ROOTS_FILE_ENV.to_owned(),
        step_output(
            dynamic_id(QualificationCacheStepStage::Roots, layer, owner_id)?,
            "root_observation_path",
        ),
    );
    let runtime_phase = if layer == QualificationCacheLayer::TaskResult {
        QualificationCacheRuntimePhase::BeforeImport
    } else {
        QualificationCacheRuntimePhase::AfterImport
    };
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE".to_owned(),
        runtime_phase.as_env_value().to_owned(),
    );
    let mut step = internal_dynamic_step(
        "Export qualification cache payload",
        "export-qualification-cache-payload-v1",
        StepRole::QualificationCachePayloadExport,
        ids.export,
        env,
    )?;
    step.condition = Some(format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{}.outputs.active == 'true'",
        ids.bind.as_string()
    ));
    Ok(step)
}

fn save_action(layer: QualificationCacheLayer, ids: &LayerIds) -> Result<Step, OrchestratorError> {
    let mut action = cache_action_step(
        false,
        ids.save,
        layer,
        ids.bind,
        "save_key",
        "save_stage_path",
    )?;
    action.condition = Some(format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{}.outputs.active == 'true' && steps.{}.outputs.save_requested == 'true' && steps.{}.outputs.save_key != ''",
        ids.bind.as_string(),
        ids.export.as_string(),
        ids.bind.as_string()
    ));
    Ok(action)
}

fn layer_record(
    owner_id: &str,
    layer: QualificationCacheLayer,
    ids: &LayerIds,
) -> Result<Step, OrchestratorError> {
    let env = layer_record_env(owner_id, layer, ids)?;
    internal_dynamic_step(
        "Record qualification cache layer evidence",
        "record-qualification-cache-owner-layer-v1",
        StepRole::QualificationCacheOwnerLayerEvidence,
        ids.record,
        env,
    )
}

fn layer_record_env(
    owner_id: &str,
    layer: QualificationCacheLayer,
    ids: &LayerIds,
) -> Result<std::collections::BTreeMap<String, String>, OrchestratorError> {
    let mut env = owner_layer_env(owner_id, layer);
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE".to_owned(),
        "before_import".to_owned(),
    );
    env.insert(
        RUNTIME_FILE_ENV.to_owned(),
        step_output(
            dynamic_id(
                QualificationCacheStepStage::RuntimeIdentity,
                layer,
                owner_id,
            )?,
            "runtime_identity_path",
        ),
    );
    if layer != QualificationCacheLayer::TaskResult {
        env.insert(
            AFTER_RUNTIME_FILE_ENV.to_owned(),
            step_output(
                dynamic_id(
                    QualificationCacheStepStage::RuntimeIdentityAfterImport,
                    layer,
                    owner_id,
                )?,
                "runtime_identity_path",
            ),
        );
    }
    env.insert(
        ROOTS_FILE_ENV.to_owned(),
        step_output(
            dynamic_id(QualificationCacheStepStage::Roots, layer, owner_id)?,
            "root_observation_path",
        ),
    );
    insert_layer_action_outputs(&mut env, ids, owner_id, layer)?;
    Ok(env)
}

fn insert_layer_action_outputs(
    env: &mut std::collections::BTreeMap<String, String>,
    ids: &LayerIds,
    owner_id: &str,
    layer: QualificationCacheLayer,
) -> Result<(), OrchestratorError> {
    for (name, output) in [
        ("EXPORT_EXPORT_RESULT", "export_result"),
        ("EXPORT_STATE_DIGEST", "state_digest"),
        ("EXPORT_SAVE_REQUESTED", "save_requested"),
    ] {
        env.insert(
            format!("VELNOR_QUALIFICATION_CACHE_{name}"),
            step_output(ids.export, output),
        );
    }
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION".to_owned(),
        format!(
            "${{{{ steps.{}.outputs.save_requested == 'true' && steps.{}.outcome || '' }}}}",
            ids.export.as_string(),
            ids.save.as_string()
        ),
    );
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_ACTIVE".to_owned(),
        step_output(ids.bind, "active"),
    );
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_SAVE_KEY".to_owned(),
        step_output(ids.bind, "save_key"),
    );
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_RESTORE_RESULT".to_owned(),
        step_output(
            dynamic_id(QualificationCacheStepStage::Import, layer, owner_id)?,
            "restore_result",
        ),
    );
    let import = dynamic_id(QualificationCacheStepStage::Import, layer, owner_id)?;
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_HIT".to_owned(),
        step_output(import, "restore_action_hit"),
    );
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY".to_owned(),
        step_output(import, "restore_action_matched_key"),
    );
    Ok(())
}

fn owner_aggregate(
    owner_id: &str,
    evidence: Vec<(QualificationCacheLayer, QualificationCacheStepId)>,
) -> Result<Step, OrchestratorError> {
    let aggregate = dynamic_id(
        QualificationCacheStepStage::OwnerAggregate,
        QualificationCacheLayer::TaskResult,
        owner_id,
    )?;
    let mut env = owner_layer_env(owner_id, QualificationCacheLayer::TaskResult);
    for (layer, id) in evidence {
        let suffix = match layer {
            QualificationCacheLayer::MbxObjects => "MBX_OBJECTS",
            QualificationCacheLayer::CargoSources => "CARGO_SOURCES",
            QualificationCacheLayer::MiseTools => "MISE_TOOLS",
            QualificationCacheLayer::TofuProviders => "TOFU_PROVIDERS",
            QualificationCacheLayer::TaskResult => "TASK_RESULT",
        };
        env.insert(
            format!("VELNOR_QUALIFICATION_CACHE_LAYER_{suffix}_EVIDENCE"),
            step_output(id, "owner_layer_evidence_path"),
        );
    }
    internal_dynamic_step(
        "Aggregate qualification cache owner evidence",
        "aggregate-qualification-cache-owner-v1",
        StepRole::QualificationCacheOwnerAggregate,
        aggregate,
        env,
    )
}
