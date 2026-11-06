// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::crate_job::{
    TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX, TOOLS_PRODUCER_JOB_ID_PREFIX,
};
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_JOB_ID_PREFIX;
use velnor_actions_contract::workflow::{
    CacheWriter, QUALIFICATION_CACHE_WRITER_IDS, QUALIFICATION_WRITER_GATE,
    SOURCES_PRODUCER_JOB_ID, WORKFLOW_CACHE_MODE, WRITER_CACHE_MODE, WRITER_MISE_ENV,
};
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::schema2::QUALIFICATION_WORKFLOW;

use std::collections::BTreeMap;

use super::rust_install_audit::parse::step_lists;
use crate::OrchestratorError;

const WORKFLOW_DIR: &str = ".github/workflows/";
const BACKGROUND_KEY: &str = "background";

struct JobHeader {
    id: String,
    cache_mode: Option<String>,
    condition: Option<String>,
    env: BTreeMap<String, String>,
}

struct Scan {
    workflow_modes: Vec<String>,
    workflow_env: BTreeMap<String, String>,
    jobs: Vec<JobHeader>,
}

enum Role {
    Producer(CacheWriter),
    Qualification,
}

pub(crate) fn audit(files: &[RenderedFile]) -> Result<(), OrchestratorError> {
    for file in files {
        let workflow = file.path.starts_with(WORKFLOW_DIR)
            && std::path::Path::new(&file.path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("yml"));
        if !workflow {
            continue;
        }
        let scan = scan(&file.bytes).map_err(|problem| {
            violation(
                &file.path,
                "workflow",
                &format!("cache_writer_invalid_yaml:{problem}"),
            )
        })?;
        if scan.workflow_modes != [WORKFLOW_CACHE_MODE] {
            return Err(violation(
                &file.path,
                "workflow",
                "workflow_cache_mode_not_read",
            ));
        }
        for job in &scan.jobs {
            let Some(mode) = job.cache_mode.as_deref() else {
                continue;
            };
            if mode == WORKFLOW_CACHE_MODE {
                continue;
            }
            check_writer(file, job, mode)?;
            check_mise_env(job, &scan.workflow_env)?;
        }
    }
    Ok(())
}

fn check_writer(file: &RenderedFile, job: &JobHeader, mode: &str) -> Result<(), OrchestratorError> {
    let path = file.path.as_str();
    let id = job.id.as_str();
    let Some(role) = role_of(path, id) else {
        return Err(violation(path, id, "cache_writer_undeclared"));
    };
    let expected = match role {
        Role::Qualification => WRITER_CACHE_MODE,
        Role::Producer(writer) => writer.mode(),
    };
    if mode != expected {
        return Err(violation(path, id, "cache_writer_mode_not_write"));
    }
    let condition = job.condition.as_deref().unwrap_or_default();
    match role {
        Role::Qualification => {
            if !condition.contains(QUALIFICATION_WRITER_GATE) {
                return Err(violation(path, id, "cache_writer_not_branch_scoped"));
            }
        }
        Role::Producer(writer) => {
            if !condition.contains(&writer.condition()) {
                return Err(violation(path, id, "cache_writer_not_trusted_gate"));
            }
            check_producer_steps(file, id, writer)?;
        }
    }
    Ok(())
}

fn check_mise_env(
    job: &JobHeader,
    workflow_env: &BTreeMap<String, String>,
) -> Result<(), OrchestratorError> {
    let pinned = WRITER_MISE_ENV.iter().all(|(key, value)| {
        job.env
            .get(*key)
            .or_else(|| workflow_env.get(*key))
            .is_some_and(|actual| actual == value)
    });
    if pinned {
        return Ok(());
    }
    Err(OrchestratorError::Contract {
        problem: format!("cache_writer_mise_config_enabled:{}", job.id),
    })
}

fn check_producer_steps(
    file: &RenderedFile,
    id: &str,
    writer: CacheWriter,
) -> Result<(), OrchestratorError> {
    for (owner, steps) in step_lists(&file.bytes) {
        if owner != id {
            continue;
        }
        for step in &steps {
            if step.invalid_anchor {
                return Err(violation(&file.path, id, "cache_writer_unresolved_step"));
            }
            if step.fields.iter().any(|(key, _)| key == BACKGROUND_KEY) {
                return Err(violation(&file.path, id, "cache_writer_runs_task_code"));
            }
            if let Some(uses) = step.uses_value()
                && !writer.allows_uses(uses)
            {
                return Err(violation(
                    &file.path,
                    id,
                    &format!("cache_writer_third_party_uses:{uses}"),
                ));
            }
        }
    }
    Ok(())
}

fn role_of(path: &str, id: &str) -> Option<Role> {
    if path == QUALIFICATION_WORKFLOW && QUALIFICATION_CACHE_WRITER_IDS.contains(&id) {
        return Some(Role::Qualification);
    }
    if id.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX) {
        return Some(Role::Producer(CacheWriter::ToolsProducer));
    }
    if id.starts_with(TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX) {
        return Some(Role::Producer(CacheWriter::TofuProvidersProducer));
    }
    if id.starts_with(STATE_SEAL_JOB_ID_PREFIX) {
        return Some(Role::Producer(CacheWriter::StateSeal));
    }
    (id == SOURCES_PRODUCER_JOB_ID).then_some(Role::Producer(CacheWriter::SourcesProducer))
}

#[derive(serde::Deserialize)]
struct WorkflowHeader {
    #[serde(rename = "cache-mode")]
    cache_mode: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, serde_json::Value>,
    jobs: BTreeMap<String, Header>,
}

#[derive(serde::Deserialize)]
struct Header {
    #[serde(rename = "cache-mode")]
    cache_mode: Option<String>,
    #[serde(rename = "if")]
    condition: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, serde_json::Value>,
}

fn scan(text: &str) -> Result<Scan, String> {
    if text.len() > 16 * 1024 * 1024
        || !super::rust_install_audit::parse::duplicate_anchor_names(text).is_empty()
    {
        return Err("input_or_duplicate_anchor".to_owned());
    }
    let mut options = serde_saphyr::Options::default();
    options.strict_indentation = true;
    options.with_snippet = false;
    if let Some(budget) = options.budget.as_mut() {
        budget.alias_anchor_ratio_multiplier = 100;
    }
    options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
    options.reject_unsupported_tags = true;
    options.reject_tagged_field_names = true;
    let header: WorkflowHeader =
        serde_saphyr::from_str_with_options(text, options).map_err(|error| error.to_string())?;
    let jobs = header
        .jobs
        .into_iter()
        .map(|(id, job)| {
            Ok(JobHeader {
                id,
                cache_mode: job.cache_mode,
                condition: job.condition,
                env: string_env(job.env)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Scan {
        workflow_modes: header.cache_mode.into_iter().collect(),
        workflow_env: string_env(header.env)?,
        jobs,
    })
}

fn string_env(
    env: BTreeMap<String, serde_json::Value>,
) -> Result<BTreeMap<String, String>, String> {
    env.into_iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_owned()))
                .ok_or_else(|| format!("non_string_env:{key}"))
        })
        .collect()
}

fn violation(path: &str, subject: &str, rule: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("{rule}:{path}:{subject}"),
    }
}

#[cfg(test)]
#[path = "cache_authority_audit_tests.rs"]
mod tests;
