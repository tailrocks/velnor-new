use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage, StepId,
    StepRole,
};

use super::census_phase;
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

/// Replace one job's contiguous cache prelude and tail with local actions.
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
) -> Result<(Vec<LayerTail>, Option<Step>), RenderError> {
    let aggregate = steps
        .pop()
        .filter(|step| step.role == Some(StepRole::QualificationCacheOwnerAggregate));
    if aggregate.is_none() {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_owner_aggregate_missing".to_owned(),
        ));
    }
    if !steps.len().is_multiple_of(5) {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_layer_tail_shape_invalid".to_owned(),
        ));
    }
    let mut chunks = Vec::with_capacity(steps.len() / 5);
    let (groups, remainder) = steps.as_chunks::<5>();
    if !remainder.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_layer_tail_shape_invalid".to_owned(),
        ));
    }
    for group in groups {
        let layer = group[0]
            .id
            .and_then(|id| match id {
                StepId::QualificationCache(id) => Some(id.layer()),
                _ => None,
            })
            .ok_or_else(|| {
                RenderError::InvalidWorkflow(
                    "qualification_cache_layer_identity_missing".to_owned(),
                )
            })?;
        let roles = group.iter().map(|step| step.role).collect::<Vec<_>>();
        if roles
            != [
                Some(StepRole::QualificationCachePayloadExport),
                Some(StepRole::QualificationCacheCensus),
                Some(StepRole::QualificationCacheSave),
                Some(StepRole::QualificationCacheCensus),
                Some(StepRole::QualificationCacheOwnerLayerEvidence),
            ]
            || census_phase(&group[1]) != Some("save_before")
            || census_phase(&group[3]) != Some("save_after")
            || group.iter().any(|step| {
                step.id.is_none_or(|id| match id {
                    StepId::QualificationCache(id) => id.layer() != layer,
                    _ => true,
                })
            })
        {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_layer_tail_order_invalid".to_owned(),
            ));
        }
        let Some(StepId::QualificationCache(record_id)) = group[4].id else {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_owner_layer_id_invalid".to_owned(),
            ));
        };
        chunks.push(LayerTail {
            layer,
            before: group[..2].to_vec(),
            save: group[2].clone(),
            after: group[3..].to_vec(),
            record_id,
        });
    }
    Ok((chunks, aggregate))
}

pub(super) fn before_outputs(
    layer: QualificationCacheLayer,
    steps: &[Step],
) -> Result<Vec<Output>, RenderError> {
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
            Some(StepRole::QualificationCacheCensus) => {
                for (field, output) in census_fields_before_save() {
                    outputs.push(Output {
                        name: format!("cache_{}_save_before_census_{field}", layer.as_str()),
                        id,
                        field: output,
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
    let expected =
        velnor_actions_contract::workflow::qualification_cache_before_save_output_names(layer);
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
    steps: &[Step],
) -> Result<Vec<Output>, RenderError> {
    let [_, record] = steps else {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_step_set_invalid".to_owned(),
        ));
    };
    if record.role != Some(StepRole::QualificationCacheOwnerLayerEvidence) {
        return Err(RenderError::InvalidWorkflow(
            "qualification_cache_after_save_record_missing".to_owned(),
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

pub(super) fn census_fields_before_save() -> [(&'static str, &'static str); 9] {
    [
        ("phase", "census_phase"),
        ("key", "census_key"),
        ("status", "backend_status"),
        ("admitted", "backend_admitted"),
        ("cache_id", "backend_cache_id"),
        ("cache_key", "backend_cache_key"),
        ("cache_ref", "backend_cache_ref"),
        ("cache_version", "backend_cache_version"),
        ("cache_size", "backend_cache_size"),
    ]
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
        for (field, output) in census_fields() {
            outputs.push(Output {
                name: format!("cache_{}_restore_census_{field}", layer.as_str()),
                id: cache_id(QualificationCacheStepStage::RestoreCensus, layer, job_id)?,
                field: output,
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

pub(super) fn census_fields() -> [(&'static str, &'static str); 12] {
    [
        ("phase", "census_phase"),
        ("key", "census_key"),
        ("status", "backend_status"),
        ("admitted", "backend_admitted"),
        ("cache_id", "backend_cache_id"),
        ("cache_key", "backend_cache_key"),
        ("cache_ref", "backend_cache_ref"),
        ("cache_version", "backend_cache_version"),
        ("cache_size", "backend_cache_size"),
        ("action_hit", "restore_action_hit"),
        ("action_matched_key", "restore_action_matched_key"),
        ("restore_key", "restore_key"),
    ]
}

pub(super) fn cache_id(
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
    job_id: &str,
) -> Result<QualificationCacheStepId, RenderError> {
    QualificationCacheStepId::new(stage, layer, job_id).map_err(RenderError::Contract)
}
