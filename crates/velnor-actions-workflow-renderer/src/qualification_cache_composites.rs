//! Size fallback that keeps one owner's cache lifecycle in typed local actions.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::{
    QualificationCacheCompositeId, QualificationCacheCompositePhase, QualificationCacheLayer,
    QualificationCacheStepId, QualificationCacheStepStage,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::RenderError;
use crate::tree::RenderedFile;

const PREFIX: &str = "./.github/actions/qualification-cache-";
const CAMPAIGN_INPUT: &str = "qualification_campaign";

mod actions;
mod output;
mod rewrite;

use actions::{call_step, composite_file};
use output::{
    LayerTail, Output, after_outputs, before_outputs, cache_id, output_map, outputs, split_tail,
};
use rewrite::{
    inputs_for_after, inputs_from_prepare, rewrite_aggregate_reference,
    rewrite_cross_phase_outputs, rewrite_outer_save,
};

pub(crate) fn extract(
    jobs: &mut BTreeMap<String, Job>,
    already_shared: &BTreeSet<String>,
    ctx: &crate::render::RenderContext,
) -> Result<Vec<RenderedFile>, RenderError> {
    let mut files = Vec::new();
    for (job_id, job) in jobs {
        if already_shared.contains(job_id) {
            continue;
        }
        let Some(prelude) = prelude_range(&job.steps) else {
            continue;
        };
        let Some(tail) = tail_range(&job.steps) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "qualification_cache_tail_missing:{job_id}"
            )));
        };
        if prelude.end >= tail.start {
            return Err(RenderError::InvalidWorkflow(format!(
                "qualification_cache_phase_overlap:{job_id}"
            )));
        }
        let prelude_steps = job.steps[prelude.clone()].to_vec();
        let tail_steps = job.steps[tail.clone()].to_vec();
        let prepare_outputs = outputs(job_id)?;
        let prepare_map = output_map(&prepare_outputs, "prepare_outputs");
        let prepare_id = composite_id(QualificationCacheCompositePhase::Prepare, job_id)?;
        let prepare_file = composite_file(
            job_id,
            QualificationCacheCompositePhase::Prepare,
            None,
            prelude_steps,
            vec![CAMPAIGN_INPUT.to_owned()],
            &prepare_outputs,
            ctx,
        )?;
        let prepare = call_step(
            job_id,
            QualificationCacheCompositePhase::Prepare,
            None,
            prepare_id,
            BTreeMap::from([(
                CAMPAIGN_INPUT.to_owned(),
                "${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
            )]),
        )?;
        let (layers, aggregate) = split_tail(tail_steps)?;
        let mut tail_calls = Vec::with_capacity(layers.len() * 3 + 1);
        let mut layer_files = Vec::with_capacity(layers.len() * 2);
        let mut aggregate = aggregate;
        for chunk in layers {
            let built = build_layer_composites(chunk, job_id, prepare_id, &prepare_map, ctx)?;
            if let Some(aggregate) = aggregate.as_mut() {
                rewrite_aggregate_reference(aggregate, built.record_id, built.after_id)?;
            }
            tail_calls.extend(built.steps);
            layer_files.extend(built.files);
        }
        if let Some(aggregate) = aggregate {
            tail_calls.push(aggregate);
        }
        let mut replaced =
            Vec::with_capacity(job.steps.len() - prelude.len() - tail.len() + 1 + tail_calls.len());
        replaced.extend(job.steps[..prelude.start].iter().cloned());
        replaced.push(prepare);
        replaced.extend(job.steps[prelude.end..tail.start].iter().cloned());
        replaced.extend(tail_calls);
        replaced.extend(job.steps[tail.end..].iter().cloned());
        job.steps = replaced;
        files.push(prepare_file);
        files.extend(layer_files);
    }
    Ok(files)
}

struct LayerCompositeOutput {
    steps: [Step; 3],
    files: [RenderedFile; 2],
    record_id: QualificationCacheStepId,
    after_id: QualificationCacheCompositeId,
}

fn build_layer_composites(
    chunk: LayerTail,
    job_id: &str,
    prepare_id: QualificationCacheCompositeId,
    prepare_map: &BTreeMap<String, String>,
    ctx: &crate::render::RenderContext,
) -> Result<LayerCompositeOutput, RenderError> {
    let layer = chunk.layer;
    let before_id =
        layer_composite_id(QualificationCacheCompositePhase::BeforeSave, layer, job_id)?;
    let before_outputs = before_outputs(layer, &chunk.before)?;
    let mut before_input_names =
        velnor_actions_contract::workflow::qualification_cache_layer_prepare_input_names(layer);
    before_input_names.push(CAMPAIGN_INPUT.to_owned());
    let before_map = output_map(&before_outputs, "before_outputs");
    let transformed_before = rewrite_cross_phase_outputs(chunk.before, prepare_map)?;
    let before_file = composite_file(
        job_id,
        QualificationCacheCompositePhase::BeforeSave,
        Some(layer),
        transformed_before,
        before_input_names,
        &before_outputs,
        ctx,
    )?;
    let before_call = call_step(
        job_id,
        QualificationCacheCompositePhase::BeforeSave,
        Some(layer),
        before_id,
        inputs_from_prepare(prepare_id),
    )?;

    let save_id = cache_id(QualificationCacheStepStage::Save, layer, job_id)?;
    let mut save = chunk.save;
    rewrite_outer_save(&mut save, layer, job_id, prepare_id, before_id)?;
    let save_outcome = format!("${{{{ steps.{}.outcome }}}}", save_id.as_string());
    let after_id = layer_composite_id(QualificationCacheCompositePhase::AfterSave, layer, job_id)?;
    let after_outputs = after_outputs(layer, &chunk.after)?;
    let mut after_input_names =
        velnor_actions_contract::workflow::qualification_cache_after_save_input_names(layer);
    after_input_names.push(CAMPAIGN_INPUT.to_owned());
    let mut after_map = prepare_map.clone();
    after_map.extend(before_map);
    after_map.insert(
        format!("steps.{}.outcome", save_id.as_string()),
        format!("inputs.cache_{}_save_action_outcome", layer.as_str()),
    );
    let transformed_after = rewrite_cross_phase_outputs(chunk.after, &after_map)?;
    let after_file = composite_file(
        job_id,
        QualificationCacheCompositePhase::AfterSave,
        Some(layer),
        transformed_after,
        after_input_names,
        &after_outputs,
        ctx,
    )?;
    let after_call = call_step(
        job_id,
        QualificationCacheCompositePhase::AfterSave,
        Some(layer),
        after_id,
        inputs_for_after(layer, prepare_id, before_id, save_id, job_id, &save_outcome)?,
    )?;
    Ok(LayerCompositeOutput {
        steps: [before_call, save, after_call],
        files: [before_file, after_file],
        record_id: chunk.record_id,
        after_id,
    })
}

fn prelude_range(steps: &[Step]) -> Option<std::ops::Range<usize>> {
    let start = steps.iter().position(is_prepare_step)?;
    let end = steps.iter().rposition(is_prepare_step)? + 1;
    steps[start..end]
        .iter()
        .all(is_prepare_step)
        .then_some(start..end)
}

fn tail_range(steps: &[Step]) -> Option<std::ops::Range<usize>> {
    let start = steps.iter().position(is_finish_step)?;
    let end = steps.iter().rposition(is_finish_step)? + 1;
    steps[start..end]
        .iter()
        .all(is_finish_step)
        .then_some(start..end)
}

fn is_prepare_step(step: &Step) -> bool {
    match step.role {
        Some(StepRole::QualificationCacheCensus) => census_phase(step) == Some("restore"),
        Some(
            StepRole::QualificationRuntimeIdentity
            | StepRole::QualificationCacheRoots
            | StepRole::QualificationCacheBind
            | StepRole::QualificationCacheRestore
            | StepRole::QualificationCachePayloadImport
            | StepRole::QualificationCacheRuntimeIdentityAfterImport,
        ) => true,
        _ => false,
    }
}

fn is_finish_step(step: &Step) -> bool {
    match step.role {
        Some(StepRole::QualificationCacheCensus) => {
            matches!(census_phase(step), Some("save_before" | "save_after"))
        }
        Some(
            StepRole::QualificationCachePayloadExport
            | StepRole::QualificationCacheSave
            | StepRole::QualificationCacheOwnerLayerEvidence
            | StepRole::QualificationCacheOwnerAggregate,
        ) => true,
        _ => false,
    }
}

fn census_phase(step: &Step) -> Option<&str> {
    let StepKind::Internal { env, .. } = &step.kind else {
        return None;
    };
    env.get("VELNOR_QUALIFICATION_CACHE_CENSUS_PHASE")
        .map(String::as_str)
}

fn composite_id(
    phase: QualificationCacheCompositePhase,
    job_id: &str,
) -> Result<QualificationCacheCompositeId, RenderError> {
    QualificationCacheCompositeId::new(phase, job_id).map_err(RenderError::Contract)
}

fn layer_composite_id(
    phase: QualificationCacheCompositePhase,
    layer: QualificationCacheLayer,
    job_id: &str,
) -> Result<QualificationCacheCompositeId, RenderError> {
    QualificationCacheCompositeId::for_layer(phase, layer, job_id).map_err(RenderError::Contract)
}
