// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::{ir::Job, step::StepKind};
use super::{
    EmitLane, HOSTED_SUFFIX, LaneClass, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
    SCALE_SUFFIX,
};
use crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION;

pub(super) fn retarget(
    job: &Job,
    runs_on: Option<&str>,
    source_id: &str,
    output_id: &str,
    class: LaneClass,
    lane: EmitLane,
    map: &BTreeMap<String, Vec<String>>,
) -> Result<Job, crate::ContractError> {
    let mut next = job.clone();
    if let Some(token) = runs_on {
        token.clone_into(&mut next.runs_on);
    }
    if next.check_runner.is_some() {
        next.condition = retarget_check_condition(
            job.condition.as_deref(),
            source_id,
            output_id,
            runs_on.is_some(),
        )?;
    }
    retarget_check_identity(&mut next, source_id, output_id, lane)?;
    next.needs = expand_needs(&job.needs, map, class, lane);
    Ok(next)
}

fn retarget_check_condition(
    original: Option<&str>,
    source_id: &str,
    output_id: &str,
    scale: bool,
) -> Result<Option<String>, crate::ContractError> {
    let source_gate = super::super::jobs::not_affected_gate(source_id);
    let source_both = format!("({source_gate}) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})");
    let gate = super::super::jobs::not_affected_gate(output_id);
    match original {
        None => Ok(scale.then(|| EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned())),
        Some(value) if value == source_gate || value == source_both => Ok(Some(if scale {
            format!("({gate}) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})")
        } else {
            gate
        })),
        Some(value) if value == EPHEMERAL_CHECK_ADMISSION_CONDITION => Ok(Some(value.to_owned())),
        Some(value) if source_id == output_id && !scale => Ok(Some(value.to_owned())),
        Some(_) => Err(crate::ContractError::identity(
            "job.condition",
            "check_lane_condition_unsupported",
        )),
    }
}

fn retarget_check_identity(
    job: &mut Job,
    source_id: &str,
    output_id: &str,
    lane: EmitLane,
) -> Result<(), crate::ContractError> {
    for step in &mut job.steps {
        match &mut step.kind {
            StepKind::Shell { env, .. } | StepKind::Internal { env, .. } => {
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
            }
            StepKind::Action { with, .. } => {
                if let Some(name) = with.get_mut("name")
                    && name.starts_with("velnor-crate-")
                    && name.ends_with(&format!("-{source_id}"))
                {
                    let prefix_len = name.len() - source_id.len();
                    name.truncate(prefix_len);
                    name.push_str(output_id);
                }
            }
            StepKind::TaskExecution {
                task_id,
                matrix_id,
                matrix_key,
                ..
            } => {
                let variant = match lane {
                    EmitLane::Single => None,
                    EmitLane::Hosted => Some(super::LaneVariant::Hosted),
                    EmitLane::Scale => Some(super::LaneVariant::ScaleSet),
                };
                if let Some(variant) = variant {
                    let (id, key) = super::super::matrix_entry::project_task_matrix_identity(
                        task_id, matrix_id, matrix_key, variant,
                    )?;
                    *matrix_id = id;
                    *matrix_key = key;
                }
            }
            StepKind::NativeCheckExecution { .. } => {}
        }
    }
    Ok(())
}

pub(super) fn expand_needs(
    needs: &[String],
    map: &BTreeMap<String, Vec<String>>,
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
