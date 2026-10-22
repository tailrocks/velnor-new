use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage, StepId,
    StepRole,
};

use crate::RenderError;

#[derive(Clone)]
pub(super) struct Output {
    pub(super) name: String,
    pub(super) id: QualificationCacheStepId,
    pub(super) field: &'static str,
}

pub(super) struct LayerTail {
    pub(super) layer: QualificationCacheLayer,
    pub(super) before: Vec<Step>,
    pub(super) save: Step,
    pub(super) after: Vec<Step>,
    pub(super) record_id: QualificationCacheStepId,
}

/// Map step output references to the specified composite action input fields.
pub(super) fn output_map(outputs: &[Output], input: &str) -> BTreeMap<String, String> {
    outputs
        .iter()
        .map(|output| {
            (
                format!("steps.{}.outputs.{}", output.id.as_string(), output.field),
                format!("fromJSON(inputs.{input}).{}", output.name),
            )
        })
        .collect()
}

pub(super) fn split_tail(
    mut steps: Vec<Step>,
    owner_id: &str,
) -> Result<(Vec<LayerTail>, Option<Step>), RenderError> {
    let aggregate = steps
        .pop()
        .filter(|step| step.role == Some(StepRole::QualificationCacheOwnerAggregate));
    if aggregate.as_ref().is_none_or(|step| {
        !step.id.is_some_and(|id| match id {
            StepId::QualificationCache(id) => {
                id.stage() == QualificationCacheStepStage::OwnerAggregate && id.belongs_to(owner_id)
            }
            _ => false,
        })
    }) {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_owner_aggregate_missing".to_owned(),
        ));
    }
    if !steps.len().is_multiple_of(3) {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_layer_tail_shape_invalid".to_owned(),
        ));
    }
    let mut chunks = Vec::with_capacity(steps.len() / 3);
    let (groups, remainder) = steps.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_layer_tail_shape_invalid".to_owned(),
        ));
    }
    for group in groups {
        chunks.push(split_layer_group(group, owner_id)?);
    }
    Ok((chunks, aggregate))
}

fn split_layer_group(group: &[Step; 3], owner_id: &str) -> Result<LayerTail, RenderError> {
    let layer = group[0]
        .id
        .and_then(|id| match id {
            StepId::QualificationCache(id) => Some(id.layer()),
            _ => None,
        })
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("qualification_cache_layer_identity_missing".to_owned())
        })?;
    let roles = group.iter().map(|step| step.role).collect::<Vec<_>>();
    if roles
        != [
            Some(StepRole::QualificationCachePayloadExport),
            Some(StepRole::QualificationCacheSave),
            Some(StepRole::QualificationCacheOwnerLayerEvidence),
        ]
        || !cache_step_matches(
            &group[0],
            QualificationCacheStepStage::Export,
            layer,
            owner_id,
        )
        || !cache_step_matches(
            &group[1],
            QualificationCacheStepStage::Save,
            layer,
            owner_id,
        )
        || !cache_step_matches(
            &group[2],
            QualificationCacheStepStage::OwnerLayerEvidence,
            layer,
            owner_id,
        )
    {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_layer_tail_order_invalid".to_owned(),
        ));
    }
    let Some(StepId::QualificationCache(record_id)) = group[2].id else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_owner_layer_id_invalid".to_owned(),
        ));
    };
    Ok(LayerTail {
        layer,
        before: vec![group[0].clone()],
        save: group[1].clone(),
        after: vec![group[2].clone()],
        record_id,
    })
}

pub(super) fn before_outputs(
    layer: QualificationCacheLayer,
    owner_id: &str,
    steps: &[Step],
) -> Result<Vec<Output>, RenderError> {
    if steps.len() != 1
        || !cache_step_matches(
            &steps[0],
            QualificationCacheStepStage::Export,
            layer,
            owner_id,
        )
    {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_before_save_step_invalid".to_owned(),
        ));
    }
    let mut outputs = Vec::new();
    for step in steps {
        let Some(StepId::QualificationCache(id)) = step.id else {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_before_save_id_missing".to_owned(),
            ));
        };
        match step.role {
            Some(StepRole::QualificationCachePayloadExport) => {
                for field in ["export_result", "state_digest", "save_requested"] {
                    outputs.push(Output {
                        name: format!("cache_{}_{}", layer.as_str(), field),
                        id,
                        field,
                    });
                }
            }
            _ => {
                return Err(RenderError::InvalidWorkflow(
                    "qualification_cache_before_save_step_invalid".to_owned(),
                ));
            }
        }
    }
    outputs.sort_by(|left, right| left.name.cmp(&right.name));
    let mut expected = ["export_result", "state_digest", "save_requested"]
        .into_iter()
        .map(|field| format!("cache_{}_{}", layer.as_str(), field))
        .collect::<Vec<_>>();
    expected.sort();
    if outputs
        .iter()
        .map(|output| output.name.clone())
        .collect::<Vec<_>>()
        != expected
    {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_before_save_output_set_invalid".to_owned(),
        ));
    }
    Ok(outputs)
}

pub(super) fn after_outputs(
    layer: QualificationCacheLayer,
    owner_id: &str,
    steps: &[Step],
) -> Result<Vec<Output>, RenderError> {
    let [record] = steps else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_step_set_invalid".to_owned(),
        ));
    };
    if record.role != Some(StepRole::QualificationCacheOwnerLayerEvidence) {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_record_missing".to_owned(),
        ));
    }
    if !cache_step_matches(
        record,
        QualificationCacheStepStage::OwnerLayerEvidence,
        layer,
        owner_id,
    ) {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_record_id_missing".to_owned(),
        ));
    }
    let Some(StepId::QualificationCache(id)) = record.id else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_record_id_missing".to_owned(),
        ));
    };
    let outputs = vec![Output {
        name: format!("cache_{}_owner_evidence_path", layer.as_str()),
        id,
        field: "owner_layer_evidence_path",
    }];
    if outputs
        .iter()
        .map(|output| output.name.clone())
        .collect::<Vec<_>>()
        != velnor_actions_contract::workflow::qualification_cache_after_save_output_names(layer)
    {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_output_set_invalid".to_owned(),
        ));
    }
    Ok(outputs)
}

fn cache_step_matches(
    step: &Step,
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
    owner_id: &str,
) -> bool {
    step.id.is_some_and(|id| match id {
        StepId::QualificationCache(id) => {
            id.stage() == stage && id.layer() == layer && id.belongs_to(owner_id)
        }
        _ => false,
    })
}

pub(super) fn outputs(job_id: &str) -> Result<Vec<Output>, RenderError> {
    let mut outputs = Vec::new();
    for layer in layers() {
        let names = [
            (
                "runtime_before",
                QualificationCacheStepStage::RuntimeIdentity,
                "runtime_identity_path",
            ),
            (
                "roots",
                QualificationCacheStepStage::Roots,
                "root_observation_path",
            ),
            ("active", QualificationCacheStepStage::Bind, "active"),
            ("save_key", QualificationCacheStepStage::Bind, "save_key"),
            (
                "save_stage_path",
                QualificationCacheStepStage::Bind,
                "save_stage_path",
            ),
            (
                "restore_result",
                QualificationCacheStepStage::Import,
                "restore_result",
            ),
        ];
        for (field, stage, output) in names {
            outputs.push(Output {
                name: format!("cache_{}_{}", layer.as_str(), field),
                id: cache_id(stage, layer, job_id)?,
                field: output,
            });
        }
        if layer != QualificationCacheLayer::TaskResult {
            outputs.push(Output {
                name: format!("cache_{}_runtime_after", layer.as_str()),
                id: cache_id(
                    QualificationCacheStepStage::RuntimeIdentityAfterImport,
                    layer,
                    job_id,
                )?,
                field: "runtime_identity_path",
            });
        }
        let import_id = cache_id(QualificationCacheStepStage::Import, layer, job_id)?;
        for (field, action_output) in [
            ("action_hit", "restore_action_hit"),
            ("action_matched_key", "restore_action_matched_key"),
        ] {
            outputs.push(Output {
                name: format!("cache_{}_restore_census_{field}", layer.as_str()),
                id: import_id,
                field: action_output,
            });
        }
    }
    outputs.sort_by(|left, right| left.name.cmp(&right.name));
    let expected = velnor_actions_contract::workflow::qualification_cache_composite_output_names();
    if outputs
        .iter()
        .map(|output| output.name.clone())
        .collect::<Vec<_>>()
        != expected
    {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_composite_output_set_invalid".to_owned(),
        ));
    }
    Ok(outputs)
}

pub(super) fn layers() -> [QualificationCacheLayer; 5] {
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
}

pub(super) fn cache_id(
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
    job_id: &str,
) -> Result<QualificationCacheStepId, RenderError> {
    QualificationCacheStepId::new(stage, layer, job_id).map_err(RenderError::Contract)
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
