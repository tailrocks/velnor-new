use super::super::ir::Job;
use super::super::step_identity::{
    QualificationCacheCompositeId, QualificationCacheStepId, StepId,
};
use super::{
    EmitLane, HOSTED_SUFFIX, LaneClass, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
    SCALE_SUFFIX,
};
use crate::config::{CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION};
use crate::errors::ContractError;
use std::collections::BTreeMap;

const QUALIFICATION_CACHE_OWNER_ID_ENV: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";

#[path = "lanes/emit/cache_references.rs"]
mod cache_references;
use cache_references::{
    cache_owner_references, retarget_owner_env, rewrite_condition_references,
    rewrite_expression_map, rewrite_expression_references,
};

pub(super) fn retarget(
    job: &Job,
    runs_on: Option<&str>,
    source_id: &str,
    output_id: &str,
    class: LaneClass,
    lane: EmitLane,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Job, ContractError> {
    let references = cache_owner_references(job, source_id, output_id)?;
    let mut next = job.clone();
    if let Some(token) = runs_on {
        token.clone_into(&mut next.runs_on);
        if let Some(runner) = &next.check_runner {
            next.condition = Some(
                if runner.executor == CheckExecutor::Hosted
                    && runner.platform == CheckPlatform::LinuxX64
                {
                    match next.condition.as_deref() {
                        Some(condition) => {
                            format!("({condition}) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})")
                        }
                        None => EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned(),
                    }
                } else {
                    EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()
                },
            );
        }
    }
    retarget_check_identity(&mut next, source_id, output_id, lane, &references)?;
    next.needs = expand_needs(&job.needs, map, class, lane);
    Ok(next)
}

fn retarget_check_identity(
    job: &mut Job,
    source_id: &str,
    output_id: &str,
    lane: EmitLane,
    references: &BTreeMap<String, String>,
) -> Result<(), ContractError> {
    for step in &mut job.steps {
        let is_cache_step = matches!(
            step.id,
            Some(StepId::QualificationCache(_) | StepId::QualificationCacheComposite(_))
        );
        if let Some(id) = step.id.as_mut() {
            match id {
                StepId::QualificationCache(identity) => {
                    *identity = QualificationCacheStepId::new(
                        identity.stage(),
                        identity.layer(),
                        output_id,
                    )?;
                }
                StepId::QualificationCacheComposite(identity) => {
                    *identity = match identity.layer() {
                        Some(layer) => QualificationCacheCompositeId::for_layer(
                            identity.phase(),
                            layer,
                            output_id,
                        )?,
                        None => QualificationCacheCompositeId::new(identity.phase(), output_id)?,
                    };
                }
                _ => {}
            }
        }
        if let Some(condition) = &mut step.condition {
            rewrite_condition_references(condition, references)?;
        }
        retarget_step_payload(step, source_id, output_id, lane, references, is_cache_step)?;
    }
    Ok(())
}

fn retarget_step_payload(
    step: &mut super::super::step::Step,
    source_id: &str,
    output_id: &str,
    lane: EmitLane,
    references: &BTreeMap<String, String>,
    is_cache_step: bool,
) -> Result<(), ContractError> {
    match &mut step.kind {
        super::super::step::StepKind::Shell { run, env } => {
            if let Some(job_id) = env.get_mut(NAMED_CHECK_JOB_ID_ENV)
                && job_id == source_id
            {
                output_id.clone_into(job_id);
            }
            if let Some(variant) = env.get_mut(NAMED_CHECK_LANE_VARIANT_ENV) {
                match lane {
                    EmitLane::Hosted => "hosted".clone_into(variant),
                    EmitLane::Scale => "scale_set".clone_into(variant),
                    EmitLane::Single => {}
                }
            }
            for argument in run {
                rewrite_expression_references(argument, references)?;
            }
            if is_cache_step {
                retarget_owner_env(env, source_id, output_id);
            }
            rewrite_expression_map(env, references)?;
        }
        super::super::step::StepKind::Action { with, env, .. } => {
            if let Some(name) = with.get_mut("name")
                && name.starts_with("velnor-crate-")
                && name.ends_with(&format!("-{source_id}"))
            {
                let prefix_len = name.len() - source_id.len();
                name.truncate(prefix_len);
                name.push_str(output_id);
            }
            if is_cache_step {
                retarget_owner_env(env, source_id, output_id);
            }
            rewrite_expression_map(with, references)?;
            rewrite_expression_map(env, references)?;
        }
        super::super::step::StepKind::Internal { env, .. } => {
            if is_cache_step {
                retarget_owner_env(env, source_id, output_id);
            }
            rewrite_expression_map(env, references)?;
        }
        // Lane expansion must not change the task execution arguments,
        // environment, or binding digest.
        super::super::step::StepKind::TaskExecution { .. }
        | super::super::step::StepKind::NativeCheckExecution { .. } => {}
    }
    Ok(())
}

fn expand_needs(
    needs: &[String],
    map: &std::collections::BTreeMap<String, Vec<String>>,
    class: LaneClass,
    lane: EmitLane,
) -> Vec<String> {
    let mut out = Vec::new();
    for need in needs {
        let targets = map.get(need).cloned().unwrap_or_else(|| vec![need.clone()]);
        if class != LaneClass::Verification || targets.len() == 1 {
            out.extend(targets);
            continue;
        }
        let suffix = match lane {
            EmitLane::Hosted => HOSTED_SUFFIX,
            EmitLane::Scale => SCALE_SUFFIX,
            EmitLane::Single => "",
        };
        if suffix.is_empty() {
            out.extend(targets);
        } else {
            out.extend(targets.into_iter().filter(|id| id.ends_with(suffix)));
        }
    }
    out
}
