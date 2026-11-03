// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_family::{
    PLAN_CACHE_OUTPUTS, PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES, PLAN_CACHE_TRUST_OUTPUT,
    SOURCES_PRODUCE_OUTPUT, SOURCES_PRODUCE_STEP_EXPR, cache_output_reserve_utf16_bytes,
    is_plan_cache_output, is_state_snapshot_output,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_CANARY_VERIFIED_OUTPUT, is_state_attested_output,
};
use velnor_actions_contract::{Job, Step, StepId, StepKind};

use crate::RenderError;

const NEEDS_PLAN: &str = "needs.plan.outputs.";
static EMPTY_ENV: std::sync::LazyLock<BTreeMap<String, String>> =
    std::sync::LazyLock::new(BTreeMap::new);

pub(crate) fn consumed(jobs: &BTreeMap<String, Job>) -> Result<Vec<(String, String)>, RenderError> {
    let mut names = BTreeSet::new();
    let texts = jobs.values().flat_map(|job| {
        let steps = job.steps.iter().flat_map(step_texts);
        job.condition.as_deref().into_iter().chain(steps)
    });
    for text in texts {
        let mut rest = text;
        while let Some(at) = rest.find(NEEDS_PLAN) {
            rest = &rest[at + NEEDS_PLAN.len()..];
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(rest.len());
            if is_plan_cache_output(&rest[..end]) {
                names.insert(rest[..end].to_owned());
            }
        }
    }
    let has_key_step = jobs.get(crate::render::PLAN_JOB_ID).is_some_and(|plan| {
        plan.steps
            .iter()
            .any(|step| step.id == Some(StepId::CacheKeys))
    });
    if has_key_step {
        names.insert(PLAN_CACHE_TRUST_OUTPUT.to_owned());
    } else if names.is_empty() {
        return Ok(Vec::new());
    } else {
        return Err(RenderError::InvalidWorkflow(
            "plan_cache_output_without_key_step".to_owned(),
        ));
    }
    let state = names
        .iter()
        .filter(|name| is_state_snapshot_output(name))
        .count();
    let attested = names
        .iter()
        .filter(|name| is_state_attested_output(name))
        .count();
    let tofu = names
        .iter()
        .filter(|name| {
            *name != SOURCES_PRODUCE_OUTPUT
                && *name != STATE_CANARY_VERIFIED_OUTPUT
                && !PLAN_CACHE_OUTPUTS.iter().any(|(output, _)| output == name)
        })
        .count()
        - state
        - attested;
    if cache_output_reserve_utf16_bytes(tofu, state, attested)
        > PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "plan_cache_outputs_exceed_reserve:{tofu}"
        )));
    }
    Ok(names
        .into_iter()
        .map(|name| {
            let expression = if name == SOURCES_PRODUCE_OUTPUT {
                SOURCES_PRODUCE_STEP_EXPR.to_owned()
            } else if is_state_attested_output(&name) || name == STATE_CANARY_VERIFIED_OUTPUT {
                format!(
                    "${{{{ steps.{}.outputs.{name} }}}}",
                    velnor_actions_contract::StepId::StateSelect.as_str()
                )
            } else {
                let step_output = PLAN_CACHE_OUTPUTS
                    .iter()
                    .find(|(output, _)| *output == name)
                    .map_or_else(|| name.clone(), |(_, step)| (*step).to_owned());
                format!(
                    "${{{{ steps.{}.outputs.{step_output} }}}}",
                    StepId::CacheKeys.as_str()
                )
            };
            (name, expression)
        })
        .collect())
}

fn step_texts(step: &Step) -> Vec<&str> {
    let mut texts: Vec<&str> = step.condition.iter().map(String::as_str).collect();
    if let StepKind::NativeCheckExecution {
        check_id,
        task_id,
        sdk_version,
        ..
    } = &step.kind
    {
        texts.extend([check_id.as_str(), task_id.as_str(), sdk_version.as_str()]);
        return texts;
    }
    let (values, env) = match &step.kind {
        StepKind::Action { with, env, .. } => (with.values().collect::<Vec<_>>(), env),
        StepKind::Shell { run, env } => (run.iter().collect(), env),
        StepKind::TaskExecution { argv, env, .. } => (argv.iter().collect(), env),
        StepKind::NativeCheckExecution { .. } => (Vec::new(), &*EMPTY_ENV),
        StepKind::Internal { env, .. } => (Vec::new(), env),
    };
    texts.extend(values.into_iter().map(String::as_str));
    texts.extend(env.values().map(String::as_str));
    texts
}

pub(crate) fn output_entries(cache: &[(String, String)]) -> Vec<(String, crate::yaml::Yaml)> {
    cache
        .iter()
        .map(|(name, expression)| (name.clone(), crate::yaml::Yaml::str(expression.clone())))
        .collect()
}
