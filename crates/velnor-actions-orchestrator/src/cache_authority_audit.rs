// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::crate_job::{
    TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX, TOOLS_PRODUCER_JOB_ID_PREFIX,
};
use velnor_actions_contract::workflow::{
    CacheWriter, QUALIFICATION_CACHE_WRITER_IDS, QUALIFICATION_WRITER_GATE,
    SOURCES_PRODUCER_JOB_ID, WORKFLOW_CACHE_MODE, WRITER_CACHE_MODE,
};
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::schema2::QUALIFICATION_WORKFLOW;

use crate::OrchestratorError;
use super::rust_install_audit::parse::step_lists;

const WORKFLOW_DIR: &str = ".github/workflows/";
const CACHE_MODE_KEY: &str = "cache-mode:";
const CONDITION_KEY: &str = "if:";
const BACKGROUND_KEY: &str = "background";
const JOBS_KEY: &str = "jobs:";

struct JobHeader {
    id: String,
    cache_mode: Option<String>,
    condition: Option<String>,
}

struct Scan {
    workflow_modes: Vec<String>,
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
        let scan = scan(&file.bytes);
        if scan.workflow_modes != [WORKFLOW_CACHE_MODE] {
            return Err(violation(&file.path, "workflow", "workflow_cache_mode_not_read"));
        }
        for job in &scan.jobs {
            let Some(mode) = job.cache_mode.as_deref() else {
                continue;
            };
            if mode == WORKFLOW_CACHE_MODE {
                continue;
            }
            check_writer(file, job, mode)?;
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
    if mode != WRITER_CACHE_MODE {
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
    (id == SOURCES_PRODUCER_JOB_ID).then_some(Role::Producer(CacheWriter::SourcesProducer))
}

fn scan(text: &str) -> Scan {
    let mut scan = Scan {
        workflow_modes: Vec::new(),
        jobs: Vec::new(),
    };
    let mut in_jobs = false;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let content = line.trim();
        if indent == 0 {
            in_jobs = content == JOBS_KEY;
            if let Some(value) = content.strip_prefix(CACHE_MODE_KEY) {
                scan.workflow_modes.push(scalar(value));
            }
            continue;
        }
        if !in_jobs {
            continue;
        }
        if indent == 2 && content.contains(':') {
            scan.jobs.push(JobHeader {
                id: content.split(':').next().unwrap_or_default().to_owned(),
                cache_mode: None,
                condition: None,
            });
        } else if indent == 4
            && let Some(job) = scan.jobs.last_mut()
        {
            if let Some(value) = content.strip_prefix(CACHE_MODE_KEY) {
                job.cache_mode = Some(scalar(value));
            } else if let Some(value) = content.strip_prefix(CONDITION_KEY) {
                job.condition = Some(scalar(value));
            }
        }
    }
    scan
}

fn scalar(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

fn violation(path: &str, subject: &str, rule: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("{rule}:{path}:{subject}"),
    }
}
