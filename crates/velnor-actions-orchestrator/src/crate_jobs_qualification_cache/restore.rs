use super::{
    CENSUS_PHASE_ENV, LAYERS, RESTORE_HIT_ENV, RESTORE_MATCHED_KEY_ENV, ROOTS_FILE_ENV,
    RUNTIME_FILE_ENV, RUNTIME_IDENTITY_OPERATION, cache_action_step, dynamic_id,
    internal_dynamic_step, owner_layer_env, step_output,
};
use crate::OrchestratorError;
use velnor_actions_contract::workflow::{QualificationCacheLayer, QualificationCacheStepStage};
use velnor_actions_contract::{Step, StepRole};

/// Restore each owner layer and install its payload before task execution.
pub(super) fn owner_restore_steps(owner_id: &str) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(LAYERS.len() * 3 + 4);
    for layer in LAYERS {
        append_restore_layer_steps(owner_id, layer, &mut steps)?;
    }
    for layer in LAYERS
        .iter()
        .copied()
        .filter(|layer| *layer != QualificationCacheLayer::TaskResult)
    {
        steps.push(runtime_check_step(owner_id, layer)?);
    }
    Ok(steps)
}

fn append_restore_layer_steps(
    owner_id: &str,
    layer: QualificationCacheLayer,
    steps: &mut Vec<Step>,
) -> Result<(), OrchestratorError> {
    let bind = dynamic_id(QualificationCacheStepStage::Bind, layer, owner_id)?;
    let restore = dynamic_id(QualificationCacheStepStage::Restore, layer, owner_id)?;
    let census = dynamic_id(QualificationCacheStepStage::RestoreCensus, layer, owner_id)?;
    let import = dynamic_id(QualificationCacheStepStage::Import, layer, owner_id)?;
    steps.push(restore_action(layer, bind, restore)?);
    steps.push(restore_census(owner_id, layer, bind, restore, census)?);
    steps.push(restore_import(owner_id, layer, restore, census, import)?);
    Ok(())
}

fn restore_action(
    layer: QualificationCacheLayer,
    bind: velnor_actions_contract::workflow::QualificationCacheStepId,
    restore: velnor_actions_contract::workflow::QualificationCacheStepId,
) -> Result<Step, OrchestratorError> {
    let mut action = cache_action_step(
        true,
        restore,
        layer,
        bind,
        "restore_key",
        "restore_stage_path",
    )?;
    action.condition = Some(format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{}.outputs.active == 'true'",
        bind.as_string()
    ));
    Ok(action)
}

fn restore_census(
    owner_id: &str,
    layer: QualificationCacheLayer,
    bind: velnor_actions_contract::workflow::QualificationCacheStepId,
    restore: velnor_actions_contract::workflow::QualificationCacheStepId,
    census: velnor_actions_contract::workflow::QualificationCacheStepId,
) -> Result<Step, OrchestratorError> {
    let mut env = owner_layer_env(owner_id, layer);
    env.insert(CENSUS_PHASE_ENV.to_owned(), "restore".to_owned());
    env.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    env.insert(
        RESTORE_HIT_ENV.to_owned(),
        step_output(restore, "cache-hit"),
    );
    env.insert(
        RESTORE_MATCHED_KEY_ENV.to_owned(),
        step_output(restore, "cache-matched-key"),
    );
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_KEY".to_owned(),
        step_output(bind, "restore_key"),
    );
    internal_dynamic_step(
        "Observe qualification cache restore",
        "observe-qualification-cache-backend-v1",
        StepRole::QualificationCacheCensus,
        census,
        env,
    )
}

fn restore_import(
    owner_id: &str,
    layer: QualificationCacheLayer,
    restore: velnor_actions_contract::workflow::QualificationCacheStepId,
    census: velnor_actions_contract::workflow::QualificationCacheStepId,
    import: velnor_actions_contract::workflow::QualificationCacheStepId,
) -> Result<Step, OrchestratorError> {
    let mut env = owner_layer_env(owner_id, layer);
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
    env.insert(
        ROOTS_FILE_ENV.to_owned(),
        step_output(
            dynamic_id(QualificationCacheStepStage::Roots, layer, owner_id)?,
            "root_observation_path",
        ),
    );
    env.insert(
        RESTORE_HIT_ENV.to_owned(),
        step_output(restore, "cache-hit"),
    );
    env.insert(
        RESTORE_MATCHED_KEY_ENV.to_owned(),
        step_output(restore, "cache-matched-key"),
    );
    insert_restore_census_outputs(&mut env, census);
    internal_dynamic_step(
        "Import qualification cache payload",
        "import-qualification-cache-payload-v1",
        StepRole::QualificationCachePayloadImport,
        import,
        env,
    )
}

fn insert_restore_census_outputs(
    env: &mut std::collections::BTreeMap<String, String>,
    census: velnor_actions_contract::workflow::QualificationCacheStepId,
) {
    for (field, output) in [
        ("PHASE", "census_phase"),
        ("KEY", "census_key"),
        ("STATUS", "backend_status"),
        ("ADMITTED", "backend_admitted"),
        ("CACHE_ID", "backend_cache_id"),
        ("CACHE_KEY", "backend_cache_key"),
        ("CACHE_REF", "backend_cache_ref"),
        ("CACHE_VERSION", "backend_cache_version"),
        ("CACHE_SIZE", "backend_cache_size"),
        ("ACTION_HIT", "restore_action_hit"),
        ("ACTION_MATCHED_KEY", "restore_action_matched_key"),
    ] {
        env.insert(
            format!("VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_{field}"),
            step_output(census, output),
        );
    }
}

fn runtime_check_step(
    owner_id: &str,
    layer: QualificationCacheLayer,
) -> Result<Step, OrchestratorError> {
    let bind = dynamic_id(QualificationCacheStepStage::Bind, layer, owner_id)?;
    let before = dynamic_id(
        QualificationCacheStepStage::RuntimeIdentity,
        layer,
        owner_id,
    )?;
    let id = dynamic_id(
        QualificationCacheStepStage::RuntimeIdentityAfterImport,
        layer,
        owner_id,
    )?;
    let mut env = owner_layer_env(owner_id, layer);
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE".to_owned(),
        "after_import".to_owned(),
    );
    env.insert(
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_BEFORE_FILE".to_owned(),
        step_output(before, "runtime_identity_path"),
    );
    let mut step = internal_dynamic_step(
        "Verify qualification runtime after cache import",
        RUNTIME_IDENTITY_OPERATION,
        StepRole::QualificationCacheRuntimeIdentityAfterImport,
        id,
        env,
    )?;
    step.condition = Some(format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{}.outputs.active == 'true'",
        bind.as_string()
    ));
    Ok(step)
}
