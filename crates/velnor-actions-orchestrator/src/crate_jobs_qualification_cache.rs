//! Per-job setup for owner-scoped qualification cache operations.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage, StepId,
};
use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{OrchestratorError, QualificationCacheRuntimePhase};

const RUNTIME_IDENTITY_OPERATION: &str = "observe-qualification-cache-runtime-v1";
const ROOTS_OPERATION: &str = "observe-qualification-cache-roots-v1";
const BIND_OPERATION: &str = "bind-qualification-cache-v1";
const OWNER_ENV: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";
const LAYER_ENV: &str = "VELNOR_QUALIFICATION_CACHE_LAYER";
const RUNTIME_IDENTITY_FILE_ENV: &str = "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE";
const ROOTS_FILE_ENV: &str = "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE";
const RUNTIME_PHASE_ENV: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE";
const RUNTIME_FILE_ENV: &str = "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE";
const RESTORE_HIT_ENV: &str = "VELNOR_QUALIFICATION_CACHE_HIT";
const RESTORE_MATCHED_KEY_ENV: &str = "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY";

#[path = "crate_jobs_qualification_cache/evidence.rs"]
mod evidence;
#[path = "crate_jobs_qualification_cache/restore.rs"]
mod restore;

pub(super) use evidence::owner_evidence_steps;
use restore::owner_restore_steps;

const LAYERS: [QualificationCacheLayer; 5] = [
    QualificationCacheLayer::MbxObjects,
    QualificationCacheLayer::CargoSources,
    QualificationCacheLayer::MiseTools,
    QualificationCacheLayer::TofuProviders,
    QualificationCacheLayer::TaskResult,
];

/// Build one measured identity, root observation, and binding for each job layer.
///
/// # Errors
///
/// Returns an error when a generated owner ID or internal operation is rejected.
pub(super) fn owner_binding_steps(owner_id: &str) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(LAYERS.len() * 7 + 1);
    for layer in LAYERS {
        let identity_id = dynamic_id(
            QualificationCacheStepStage::RuntimeIdentity,
            layer,
            owner_id,
        )?;
        let roots_id = dynamic_id(QualificationCacheStepStage::Roots, layer, owner_id)?;
        let bind_id = dynamic_id(QualificationCacheStepStage::Bind, layer, owner_id)?;
        let mut identity_env = owner_layer_env(owner_id, layer);
        identity_env.insert(
            RUNTIME_PHASE_ENV.to_owned(),
            QualificationCacheRuntimePhase::BeforeImport
                .as_env_value()
                .to_owned(),
        );
        let mut identity = internal_step(
            "Observe qualification cache runtime",
            RUNTIME_IDENTITY_OPERATION,
            StepRole::QualificationRuntimeIdentity,
            identity_id,
            identity_env,
        )?;
        identity.condition = Some(qualification_condition());

        let mut roots = internal_step(
            "Observe qualification cache roots",
            ROOTS_OPERATION,
            StepRole::QualificationCacheRoots,
            roots_id,
            owner_layer_env(owner_id, layer),
        )?;
        roots.condition = Some(qualification_condition());

        let mut bind_env = owner_layer_env(owner_id, layer);
        bind_env.insert(
            RUNTIME_IDENTITY_FILE_ENV.to_owned(),
            step_output(identity_id, "runtime_identity_path"),
        );
        bind_env.insert(
            ROOTS_FILE_ENV.to_owned(),
            step_output(roots_id, "root_observation_path"),
        );
        let mut bind = internal_step(
            "Bind qualification cache owner",
            BIND_OPERATION,
            StepRole::QualificationCacheBind,
            bind_id,
            bind_env,
        )?;
        bind.condition = Some(qualification_condition());
        steps.extend([identity, roots, bind]);
    }
    steps.extend(owner_restore_steps(owner_id)?);
    Ok(steps)
}

fn cache_action_step(
    restore: bool,
    id: QualificationCacheStepId,
    layer: QualificationCacheLayer,
    bind: QualificationCacheStepId,
    key_output: &str,
    path_output: &str,
) -> Result<Step, OrchestratorError> {
    let action = if restore { "restore" } else { "save" };
    let uses = velnor_actions_actionlint::PinnedActionRef::new(
        "actions/cache",
        Some(action),
        velnor_actions_actionlint::actions::CACHE_ACTION_SHA,
        velnor_actions_actionlint::actions::CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let layer_name = match layer {
        QualificationCacheLayer::MbxObjects => "mbx-objects",
        QualificationCacheLayer::CargoSources => "sources",
        QualificationCacheLayer::MiseTools => "tools",
        QualificationCacheLayer::TofuProviders => "tofu-providers",
        QualificationCacheLayer::TaskResult => "task",
    };
    let mut step = velnor_actions_workflow_renderer::steps::cache_action_step(
        restore,
        &uses,
        layer_name,
        &step_output(bind, key_output),
        &[],
        &[step_output(bind, path_output)],
    )
    .map_err(OrchestratorError::from)?;
    step.name = format!(
        "{} qualification cache {}",
        if restore { "Restore" } else { "Save" },
        layer.as_str()
    );
    step.id = Some(StepId::QualificationCache(id));
    step.role = Some(if restore {
        StepRole::QualificationCacheRestore
    } else {
        StepRole::QualificationCacheSave
    });
    Ok(step)
}

fn internal_dynamic_step(
    name: &str,
    operation: &str,
    role: StepRole,
    id: QualificationCacheStepId,
    env: BTreeMap<String, String>,
) -> Result<Step, OrchestratorError> {
    let mut step = internal_step(name, operation, role, id, env)?;
    step.condition = Some(qualification_condition());
    Ok(step)
}

fn internal_step(
    name: &str,
    operation: &str,
    role: StepRole,
    id: QualificationCacheStepId,
    env: BTreeMap<String, String>,
) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::steps::internal_step(name, operation)
        .map_err(|error| contract_error(&error))?;
    step.id = Some(StepId::QualificationCache(id));
    step.role = Some(role);
    let StepKind::Internal {
        env: step_environment,
        ..
    } = &mut step.kind
    else {
        return Err(OrchestratorError::Contract {
            problem: "qualification_cache_internal_step_invalid".to_owned(),
        });
    };
    *step_environment = env;
    Ok(step)
}

fn dynamic_id(
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
    owner_id: &str,
) -> Result<QualificationCacheStepId, OrchestratorError> {
    QualificationCacheStepId::new(stage, layer, owner_id).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

fn owner_layer_env(owner_id: &str, layer: QualificationCacheLayer) -> BTreeMap<String, String> {
    BTreeMap::from([
        (OWNER_ENV.to_owned(), owner_id.to_owned()),
        (LAYER_ENV.to_owned(), layer.as_str().to_owned()),
        (
            RUNTIME_PHASE_ENV.to_owned(),
            QualificationCacheRuntimePhase::BeforeImport
                .as_env_value()
                .to_owned(),
        ),
    ])
}

fn step_output(id: QualificationCacheStepId, output: &str) -> String {
    format!("${{{{ steps.{}.outputs.{output} }}}}", id.as_string())
}

fn qualification_condition() -> String {
    "needs.plan.outputs.qualification_campaign != ''".to_owned()
}

fn contract_error(error: &velnor_actions_workflow_renderer::RenderError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "crate_jobs_qualification_cache_tests.rs"]
mod tests;
