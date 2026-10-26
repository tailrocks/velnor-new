// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, StepKind};

use super::{insert_job_key, matrix_invalid};
use crate::{RenderError, yaml::Yaml};

pub(crate) const SUITE_SHARDS_ENV: &str = "VELNOR_SUITE_SHARDS";
pub(crate) const SUITE_MAX_PARALLEL_ENV: &str = "VELNOR_SUITE_MAX_PARALLEL";
pub(crate) const SUITE_FAIL_FAST_ENV: &str = "VELNOR_SUITE_FAIL_FAST";
pub(crate) const SUITE_ALLOW_FAILURE_ENV: &str = "VELNOR_SUITE_ALLOW_FAILURE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuiteMatrix {
    pub(crate) shards: u32,
    pub(crate) max_parallel: Option<u32>,
    pub(crate) fail_fast: bool,
}

pub(crate) fn has_suite_matrix(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(
            &step.kind,
            StepKind::Shell { env, .. }
                if env.contains_key(SUITE_SHARDS_ENV)
                    || env.contains_key(SUITE_MAX_PARALLEL_ENV)
                    || env.contains_key(SUITE_FAIL_FAST_ENV)
        )
    })
}

pub(crate) fn suite_allow_failure_jobs_of(jobs: &BTreeMap<String, Job>) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            job.steps.iter().any(|step| {
                matches!(
                    &step.kind,
                    StepKind::Shell { env, .. }
                        if env.get(SUITE_ALLOW_FAILURE_ENV).map(String::as_str) == Some("true")
                )
            })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

pub(crate) fn suite_matrices_of(
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeMap<String, SuiteMatrix>, RenderError> {
    let mut matrices = BTreeMap::new();
    for (id, job) in jobs {
        for step in &job.steps {
            let StepKind::Shell { env, .. } = &step.kind else {
                continue;
            };
            let shards = env.get(SUITE_SHARDS_ENV);
            let max_parallel = env.get(SUITE_MAX_PARALLEL_ENV);
            let fail_fast = env.get(SUITE_FAIL_FAST_ENV);
            if shards.is_none() && max_parallel.is_none() && fail_fast.is_none() {
                continue;
            }
            let (Some(shards_raw), Some(fail_fast_raw)) = (shards, fail_fast) else {
                return Err(matrix_invalid("suite_matrix_marker_partial"));
            };
            let max_partitions = u32::from(velnor_actions_contract::MAX_SUITE_PARTITIONS);
            let shards = shards_raw
                .parse::<u32>()
                .ok()
                .filter(|count| (2..=max_partitions).contains(count))
                .ok_or_else(|| matrix_invalid("suite_matrix_bad_shards"))?;
            let fail_fast = match fail_fast_raw.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(matrix_invalid("suite_matrix_bad_fail_fast")),
            };
            let max_parallel = match max_parallel {
                Some(raw) => Some(
                    raw.parse::<u32>()
                        .ok()
                        .filter(|cap| *cap >= 1 && *cap <= shards)
                        .ok_or_else(|| matrix_invalid("suite_matrix_bad_max_parallel"))?,
                ),
                None => None,
            };
            matrices.insert(
                id.clone(),
                SuiteMatrix {
                    shards,
                    max_parallel,
                    fail_fast,
                },
            );
            break;
        }
    }
    Ok(matrices)
}

pub(crate) fn attach_suite_matrices(
    document: &mut Yaml,
    suite_matrices: &BTreeMap<String, SuiteMatrix>,
) -> Result<(), RenderError> {
    if suite_matrices.is_empty() {
        return Ok(());
    }
    let Yaml::Map(entries) = document else {
        return Err(matrix_invalid("matrix_without_document"));
    };
    let Some(Yaml::Map(jobs)) = entries
        .iter_mut()
        .find(|entry| entry.0 == "jobs")
        .map(|entry| &mut entry.1)
    else {
        return Err(matrix_invalid("matrix_without_jobs"));
    };
    for (id, matrix) in suite_matrices {
        let shards: Vec<Yaml> = (1..=matrix.shards)
            .map(|shard| Yaml::Int(shard.into()))
            .collect();
        let mut strategy_entries = vec![("fail-fast".to_owned(), Yaml::Bool(matrix.fail_fast))];
        if let Some(max_parallel) = matrix.max_parallel {
            strategy_entries.push(("max-parallel".to_owned(), Yaml::Int(max_parallel.into())));
        }
        strategy_entries.push((
            "matrix".to_owned(),
            Yaml::Map(vec![("shard".to_owned(), Yaml::Seq(shards))]),
        ));
        insert_job_key(jobs, id, "strategy", Yaml::Map(strategy_entries))?;
    }
    Ok(())
}

pub(crate) fn attach_suite_allow_failure(
    document: &mut Yaml,
    allow_failure_jobs: &BTreeSet<String>,
) -> Result<(), RenderError> {
    if allow_failure_jobs.is_empty() {
        return Ok(());
    }
    let Yaml::Map(entries) = document else {
        return Err(matrix_invalid("matrix_without_document"));
    };
    let Some(Yaml::Map(jobs)) = entries
        .iter_mut()
        .find(|entry| entry.0 == "jobs")
        .map(|entry| &mut entry.1)
    else {
        return Err(matrix_invalid("matrix_without_jobs"));
    };
    for id in allow_failure_jobs {
        insert_job_key(jobs, id, "continue-on-error", Yaml::Bool(true))?;
    }
    Ok(())
}
