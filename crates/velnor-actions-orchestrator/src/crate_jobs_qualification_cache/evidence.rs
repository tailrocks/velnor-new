use super::{
    CENSUS_PHASE_ENV, LAYERS, ROOTS_FILE_ENV, RUNTIME_FILE_ENV, cache_action_step, dynamic_id,
    internal_dynamic_step, owner_layer_env, step_output,
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
    before: QualificationCacheStepId,
    save: QualificationCacheStepId,
    after: QualificationCacheStepId,
    record: QualificationCacheStepId,
}

/// Export, save, and record each owner layer after the parallel task group.
pub(in crate::crate_jobs) fn owner_evidence_steps(
    owner_id: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(LAYERS.len() * 5 + 1);
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
    steps.push(save_census_step(
        owner_id,
        layer,
        ids.bind,
        ids.export,
        ids.before,
        "save_before",
    )?);
    steps.push(save_action(layer, &ids)?);
    steps.push(save_census_step(
        owner_id,
        layer,
        ids.bind,
        ids.export,
        ids.after,
        "save_after",
    )?);
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
        before: dynamic_id(
            QualificationCacheStepStage::SaveBeforeCensus,
            layer,
            owner_id,
        )?,
        save: dynamic_id(QualificationCacheStepStage::Save, layer, owner_id)?,
        after: dynamic_id(
            QualificationCacheStepStage::SaveAfterCensus,
            layer,
            owner_id,
        )?,
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

fn save_census_step(
    owner_id: &str,
    layer: QualificationCacheLayer,
    bind: QualificationCacheStepId,
    export: QualificationCacheStepId,
    id: QualificationCacheStepId,
    phase: &str,
) -> Result<Step, OrchestratorError> {
    let mut env = owner_layer_env(owner_id, layer);
    env.insert(CENSUS_PHASE_ENV.to_owned(), phase.to_owned());
    env.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_KEY".to_owned(),
        step_output(bind, "save_key"),
    );
    let mut step = internal_dynamic_step(
        "Observe qualification cache save state",
        "observe-qualification-cache-backend-v1",
        StepRole::QualificationCacheCensus,
        id,
        env,
    )?;
    step.condition = Some(save_requested_condition(bind, export));
    Ok(step)
}

fn save_requested_condition(
    bind: QualificationCacheStepId,
    export: QualificationCacheStepId,
) -> String {
    format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{}.outputs.active == 'true' && steps.{}.outputs.save_requested == 'true'",
        bind.as_string(),
        export.as_string()
    )
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
    insert_save_census_outputs(&mut env, "SAVE_BEFORE_CENSUS", ids.before);
    insert_save_census_outputs(&mut env, "SAVE_AFTER_CENSUS", ids.after);
    insert_restore_census_outputs(&mut env, owner_id, layer)?;
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
    Ok(())
}

fn insert_save_census_outputs(
    env: &mut std::collections::BTreeMap<String, String>,
    group: &str,
    id: QualificationCacheStepId,
) {
    for (field, output) in census_output_fields() {
        env.insert(
            format!("VELNOR_QUALIFICATION_CACHE_{group}_{field}"),
            step_output(id, output),
        );
    }
}

fn insert_restore_census_outputs(
    env: &mut std::collections::BTreeMap<String, String>,
    owner_id: &str,
    layer: QualificationCacheLayer,
) -> Result<(), OrchestratorError> {
    let id = dynamic_id(QualificationCacheStepStage::RestoreCensus, layer, owner_id)?;
    for (field, output) in census_output_fields() {
        env.insert(
            format!("VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_{field}"),
            step_output(id, output),
        );
    }
    for (field, output) in [
        ("RESTORE_KEY", "restore_key"),
        ("ACTION_HIT", "restore_action_hit"),
        ("ACTION_MATCHED_KEY", "restore_action_matched_key"),
    ] {
        env.insert(
            format!("VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_{field}"),
            step_output(id, output),
        );
    }
    Ok(())
}

fn census_output_fields() -> [(&'static str, &'static str); 9] {
    [
        ("PHASE", "census_phase"),
        ("KEY", "census_key"),
        ("STATUS", "backend_status"),
        ("ADMITTED", "backend_admitted"),
        ("CACHE_ID", "backend_cache_id"),
        ("CACHE_KEY", "backend_cache_key"),
        ("CACHE_REF", "backend_cache_ref"),
        ("CACHE_VERSION", "backend_cache_version"),
        ("CACHE_SIZE", "backend_cache_size"),
    ]
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
