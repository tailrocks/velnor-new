use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{
    QualificationCacheCompositeId, QualificationCacheCompositePhase, QualificationCacheLayer,
    QualificationCacheStepId, QualificationCacheStepStage, StepId, StepRole,
};
use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;

use super::{CAMPAIGN_INPUT, cache_id};

pub(super) fn rewrite_cross_phase_outputs(
    mut steps: Vec<Step>,
    outputs: &BTreeMap<String, String>,
) -> Result<Vec<Step>, RenderError> {
    let by_expression = outputs.clone();
    for step in &mut steps {
        validate_restore_action_inputs(step, &by_expression)?;
        if let Some(condition) = &mut step.condition {
            *condition = condition.replace(
                "needs.plan.outputs.qualification_campaign",
                "inputs.qualification_campaign",
            );
            rewrite_references(condition, &by_expression);
        }
        match &mut step.kind {
            StepKind::Internal { env, .. } | StepKind::Shell { env, .. } => {
                for value in env.values_mut() {
                    rewrite_value(value, &by_expression)?;
                }
            }
            StepKind::Action { with, env, .. } => {
                for value in with.values_mut().chain(env.values_mut()) {
                    rewrite_value(value, &by_expression)?;
                }
            }
            StepKind::TaskExecution { .. } => {
                return Err(RenderError::InvalidWorkflow(
                    "qualification_cache_task_inside_composite_tail".to_owned(),
                ));
            }
            StepKind::NativeCheckExecution { .. } => {
                return Err(RenderError::InvalidWorkflow(
                    "qualification_cache_native_check_inside_composite_tail".to_owned(),
                ));
            }
        }
    }
    Ok(steps)
}

fn validate_restore_action_inputs(
    step: &Step,
    outputs: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if step.role != Some(StepRole::QualificationCacheOwnerLayerEvidence) {
        return Ok(());
    }
    let StepKind::Internal { env, .. } = &step.kind else {
        return Ok(());
    };
    for (key, field, action_output) in [
        (
            "VELNOR_QUALIFICATION_CACHE_HIT",
            "action_hit",
            "restore_action_hit",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
            "action_matched_key",
            "restore_action_matched_key",
        ),
    ] {
        let Some(value) = env.get(key) else {
            continue;
        };
        let Some(StepId::QualificationCache(id)) = step.id else {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_restore_fact_owner_missing".to_owned(),
            ));
        };
        if id.stage() != QualificationCacheStepStage::OwnerLayerEvidence {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_restore_fact_stage_invalid".to_owned(),
            ));
        }
        let suffix = id
            .as_string()
            .strip_prefix("qualification-cache-owner-layer-evidence-")
            .map(str::to_owned)
            .ok_or_else(|| {
                RenderError::InvalidWorkflow(
                    "qualification_cache_restore_fact_owner_invalid".to_owned(),
                )
            })?;
        let expected_step = format!("qualification-cache-import-{suffix}");
        let expected_reference = format!("steps.{expected_step}.outputs.{action_output}");
        let expected_value = format!("${{{{ {expected_reference} }}}}");
        let input = format!(
            "fromJSON(inputs.prepare_outputs).cache_{}_restore_census_{field}",
            id.layer().as_str()
        );
        if value != &expected_value || outputs.get(&expected_reference) != Some(&input) {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_restore_fact_binding_invalid".to_owned(),
            ));
        }
    }
    Ok(())
}

fn rewrite_value(
    value: &mut String,
    by_expression: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    rewrite_references(value, by_expression);
    if value.contains("steps.qualification-cache-prepare-") {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_cross_phase_output_unmapped".to_owned(),
        ));
    }
    Ok(())
}

fn rewrite_references(value: &mut String, by_expression: &BTreeMap<String, String>) {
    for (expression, input) in by_expression {
        *value = value.replace(expression, input);
    }
}

pub(super) fn inputs_from_prepare(
    prepare_id: QualificationCacheCompositeId,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            CAMPAIGN_INPUT.to_owned(),
            "${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
        ),
        (
            "prepare_outputs".to_owned(),
            format!(
                "${{{{ toJSON(steps.{}.outputs) }}}}",
                prepare_id.as_string()
            ),
        ),
    ])
}

pub(super) fn inputs_for_after(
    layer: QualificationCacheLayer,
    prepare_id: QualificationCacheCompositeId,
    before_id: QualificationCacheCompositeId,
    save_id: QualificationCacheStepId,
    job_id: &str,
    save_outcome: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    let expected_prepare =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, job_id)
            .map_err(RenderError::Contract)?;
    let expected_before = QualificationCacheCompositeId::for_layer(
        QualificationCacheCompositePhase::BeforeSave,
        layer,
        job_id,
    )
    .map_err(RenderError::Contract)?;
    if prepare_id != expected_prepare || before_id != expected_before {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_composite_identity_invalid".to_owned(),
        ));
    }
    let mut inputs = BTreeMap::from([
        (
            CAMPAIGN_INPUT.to_owned(),
            "${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
        ),
        (
            "prepare_outputs".to_owned(),
            format!(
                "${{{{ toJSON(steps.{}.outputs) }}}}",
                prepare_id.as_string()
            ),
        ),
        (
            "before_outputs".to_owned(),
            format!("${{{{ toJSON(steps.{}.outputs) }}}}", before_id.as_string()),
        ),
    ]);
    inputs.insert(
        format!("cache_{}_save_action_outcome", layer.as_str()),
        save_outcome.to_owned(),
    );
    let expected_save = cache_id(QualificationCacheStepStage::Save, layer, job_id)?;
    if expected_save != save_id {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_identity_invalid".to_owned(),
        ));
    }
    Ok(inputs)
}

#[cfg(test)]
#[path = "rewrite_tests.rs"]
mod tests;

pub(super) fn rewrite_outer_save(
    step: &mut Step,
    layer: QualificationCacheLayer,
    job_id: &str,
    prepare_id: QualificationCacheCompositeId,
    before_id: QualificationCacheCompositeId,
) -> Result<(), RenderError> {
    let Some(StepId::QualificationCache(save)) = step.id else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_outer_save_id_missing".to_owned(),
        ));
    };
    if save.layer() != layer || save.stage() != QualificationCacheStepStage::Save {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_outer_save_identity_invalid".to_owned(),
        ));
    }
    let bind = cache_id(QualificationCacheStepStage::Bind, layer, job_id)?;
    let export = cache_id(QualificationCacheStepStage::Export, layer, job_id)?;
    let old_save = format!("steps.{}.outputs.save_key", bind.as_string());
    let new_save = format!(
        "steps.{}.outputs.cache_{}_save_key",
        prepare_id.as_string(),
        layer.as_str()
    );
    let old_path = format!("steps.{}.outputs.save_stage_path", bind.as_string());
    let new_path = format!(
        "steps.{}.outputs.cache_{}_save_stage_path",
        prepare_id.as_string(),
        layer.as_str()
    );
    let before_save = format!(
        "steps.{}.outputs.cache_{}_save_requested",
        before_id.as_string(),
        layer.as_str()
    );
    let old_active = format!("steps.{}.outputs.active", bind.as_string());
    let new_active = format!(
        "steps.{}.outputs.cache_{}_active",
        prepare_id.as_string(),
        layer.as_str()
    );
    let old_requested = format!("steps.{}.outputs.save_requested", export.as_string());
    if let StepKind::Action { with, .. } = &mut step.kind {
        for value in with.values_mut() {
            *value = value
                .replace(&old_save, &new_save)
                .replace(&old_path, &new_path);
        }
    } else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_outer_save_payload_invalid".to_owned(),
        ));
    }
    if let Some(condition) = &mut step.condition {
        *condition = condition
            .replace(&old_active, &new_active)
            .replace(&old_requested, &before_save)
            .replace(&old_save, &new_save);
        if condition.contains(&format!("steps.{}.outputs.", bind.as_string()))
            || condition.contains(&format!("steps.{}.outputs.", export.as_string()))
        {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_outer_save_reference_unmapped".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(super) fn rewrite_aggregate_reference(
    aggregate: &mut Step,
    record_id: QualificationCacheStepId,
    after_id: QualificationCacheCompositeId,
) -> Result<(), RenderError> {
    let old = format!(
        "steps.{}.outputs.owner_layer_evidence_path",
        record_id.as_string()
    );
    let new = format!(
        "steps.{}.outputs.cache_{}_owner_evidence_path",
        after_id.as_string(),
        record_id.layer().as_str()
    );
    let StepKind::Internal { env, .. } = &mut aggregate.kind else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_aggregate_payload_invalid".to_owned(),
        ));
    };
    for value in env.values_mut() {
        *value = value.replace(&old, &new);
    }
    Ok(())
}
