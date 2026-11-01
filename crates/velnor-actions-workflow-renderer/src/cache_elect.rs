// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{CacheMode, Job, PullRequestCachePolicy, Step, StepKind, StepRole};

use crate::RenderError;
use crate::tools_identity::{ToolsDigest, tools_digest};

/// Explicit-read (reader) branches must never be elected as cache writers.
/// Their saves would be suppressed to `false` by `dispatch_cache_boundary`
/// and then rejected by `cache_doctor`, and an untrusted job must not write
/// caches.
fn is_cache_writer_eligible(jobs: &BTreeMap<String, Job>, id: &str) -> bool {
    jobs.get(id).is_none_or(|job| {
        !job.cache_mode
            .as_ref()
            .is_some_and(CacheMode::is_explicit_read)
    })
}

pub fn elect_cache_writers(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    elect_cache_writers_with_policy(jobs, PullRequestCachePolicy::ReadOnly)
}

pub fn elect_cache_writers_with_policy(
    jobs: &mut BTreeMap<String, Job>,
    policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    let mut planned = Vec::new();
    planned.extend(plan_tools_saves(jobs, policy)?);
    planned.extend(plan_provider_saves(jobs)?);
    for (id, save) in planned {
        let Some(job) = jobs.get_mut(&id) else {
            return Err(RenderError::InvalidWorkflow(
                "cache_writer_missing".to_owned(),
            ));
        };
        job.steps.push(save);
    }
    Ok(())
}

fn plan_tools_saves(
    jobs: &BTreeMap<String, Job>,
    policy: PullRequestCachePolicy,
) -> Result<Vec<(String, Step)>, RenderError> {
    let mut by_identity: BTreeMap<ToolsDigest, Vec<String>> = BTreeMap::new();
    for (id, job) in jobs {
        let identity = tools_digest(id, job)?;
        if identity.is_none() && has_tools_save(job) {
            return Err(RenderError::InvalidWorkflow(format!(
                "tools_cache_save_orphan:{id}"
            )));
        }
        if let Some(identity) = identity {
            by_identity.entry(identity).or_default().push(id.clone());
        }
    }
    let mut elected = BTreeMap::new();
    for (digest, owners) in &by_identity {
        let trusted: Vec<&str> = owners
            .iter()
            .map(String::as_str)
            .filter(|id| is_cache_writer_eligible(jobs, id))
            .collect();
        if trusted.is_empty() {
            continue;
        }
        let winner = trusted
            .iter()
            .find(|id| **id == crate::render::PLAN_JOB_ID)
            .or_else(|| {
                trusted
                    .iter()
                    .filter(|id| {
                        jobs.get(**id).is_some_and(|job| {
                            !crate::matrix::suites::has_suite_matrix(job)
                                && !crate::matrix::bun::has_bun_matrix(job)
                                && !job.is_dependency_seed()
                        })
                    })
                    .min()
            })
            .or_else(|| trusted.iter().min())
            .copied()
            .unwrap_or_default();
        if elected.insert(winner.to_owned(), digest.clone()).is_some() {
            return Err(RenderError::InvalidWorkflow(
                "tools_cache_multiple_winner_keys".to_owned(),
            ));
        }
    }
    for (id, job) in jobs {
        let saves = tools_save_steps(job);
        if elected.contains_key(id) {
            validate_existing_tools_save(job, policy)?;
        } else if !saves.is_empty() {
            let kind = if has_tools_restore(job) {
                "tools_cache_save_not_elected"
            } else {
                "tools_cache_save_orphan"
            };
            return Err(RenderError::InvalidWorkflow(format!("{kind}:{id}")));
        }
    }
    let mut planned = Vec::new();
    for id in elected.into_keys() {
        let Some(job) = jobs.get(&id) else {
            return Err(RenderError::InvalidWorkflow(
                "tools_cache_winner_missing".to_owned(),
            ));
        };
        if tools_save_steps(job).is_empty() {
            planned.push((id, expected_tools_save(policy)?));
        }
    }
    Ok(planned)
}

fn has_tools_restore(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::ToolsCacheRestore))
}

fn has_tools_save(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::ToolsCacheSave))
}

fn tools_save_steps(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheSave))
        .collect()
}

fn expected_tools_save(policy: PullRequestCachePolicy) -> Result<Step, RenderError> {
    crate::steps::tools_cache_save_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        Some(crate::cache_p08::tools_cache_save_condition(policy)),
    )
}

fn validate_existing_tools_save(
    job: &Job,
    policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    let expected = expected_tools_save(policy)?;
    let saves = tools_save_steps(job);
    if saves.len() > 1
        || saves
            .first()
            .is_some_and(|step| !crate::cache_p08::same_step_semantics(step, &expected))
    {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_save_shape".to_owned(),
        ));
    }
    Ok(())
}

fn plan_provider_saves(jobs: &BTreeMap<String, Job>) -> Result<Vec<(String, Step)>, RenderError> {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut path_for: BTreeMap<String, String> = BTreeMap::new();
    for (id, job) in jobs {
        velnor_actions_contract::workflow::step_identity::validate_step_sequence(&job.steps, id)
            .map_err(RenderError::Contract)?;
        if let Some((key, path)) = provider_restore_entry(job) {
            if path_for.get(&key).is_some_and(|existing| existing != &path) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "tofu_cache_key_path_mismatch:{id}"
                )));
            }
            path_for.entry(key.clone()).or_insert(path);
            by_key.entry(key).or_default().push(id.clone());
        }
    }
    let mut planned = Vec::new();
    for (key, owners) in &by_key {
        let trusted: Vec<&str> = owners
            .iter()
            .map(String::as_str)
            .filter(|id| is_cache_writer_eligible(jobs, id))
            .collect();
        let Some(winner) = trusted.into_iter().min() else {
            continue;
        };
        let Some(path) = path_for.get(key) else {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_path_missing".to_owned(),
            ));
        };
        for owner in owners {
            let Some(job) = jobs.get(owner) else {
                return Err(RenderError::InvalidWorkflow(
                    "tofu_provider_owner_missing".to_owned(),
                ));
            };
            if owner.as_str() != winner && has_provider_save(job) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "tofu_provider_save_not_elected:{owner}"
                )));
            }
            validate_provider_save(job, key, path)?;
            if owner.as_str() == winner && !has_provider_save(job) {
                let mut save = crate::tofu_cache::tofu_providers_save_step()?;
                save.condition = Some(
                    velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_SAVE_CONDITION
                        .to_owned(),
                );
                planned.push((owner.clone(), save));
            }
        }
    }
    for (id, job) in jobs {
        if has_provider_save(job) && provider_restore_entry(job).is_none() {
            return Err(RenderError::InvalidWorkflow(format!(
                "tofu_provider_save_orphan:{id}"
            )));
        }
    }
    Ok(planned)
}

fn provider_restore_entry(job: &Job) -> Option<(String, String)> {
    job.steps.iter().find_map(|step| {
        if step.role != Some(StepRole::TofuProvidersRestore) {
            return None;
        }
        let StepKind::Action { uses, with, env } = &step.kind else {
            return None;
        };
        if uses != velnor_actions_contract::workflow::step_identity::TOFU_PROVIDER_ADMISSION_USES
            || !env.is_empty()
        {
            return None;
        }
        let key = with.get("cache-key")?;
        if key.trim().is_empty() {
            return None;
        }
        let path = with.get("cache-path")?;
        if !crate::tofu_cache::tofu_providers_path_ok(path) {
            return None;
        }
        Some((key.clone(), path.clone()))
    })
}

fn validate_provider_save(job: &Job, key: &str, path: &str) -> Result<(), RenderError> {
    if provider_restore_entry(job)
        .as_ref()
        .is_none_or(|(restore_key, restore_path)| restore_key != key || restore_path != path)
    {
        return Err(RenderError::InvalidWorkflow(
            "tofu_provider_save_restore_mismatch".to_owned(),
        ));
    }
    let saves: Vec<&Step> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::TofuProvidersSave))
        .collect();
    if saves.len() > 1 {
        return Err(RenderError::InvalidWorkflow(
            "tofu_provider_save_duplicate".to_owned(),
        ));
    }
    if let Some(save) = saves.first() {
        let StepKind::Action { uses, with, env } = &save.kind else {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_save_shape".to_owned(),
            ));
        };
        if !uses.starts_with("actions/cache/save@")
            || !env.is_empty()
            || with.len() != 2
            || with.get("key").map(String::as_str)
                != Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR)
            || with.get("path").map(String::as_str)
                != Some(velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_PATH_OUTPUT_EXPR)
            || save.condition.as_deref()
                != Some(velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_SAVE_CONDITION)
        {
            return Err(RenderError::InvalidWorkflow(
                "tofu_provider_save_restore_mismatch".to_owned(),
            ));
        }
        return Ok(());
    }
    Ok(())
}

fn has_provider_save(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::TofuProvidersSave))
}
