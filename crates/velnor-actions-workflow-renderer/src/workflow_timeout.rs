//! Final transport admission for every executable generated workflow job.

use std::collections::BTreeSet;

use velnor_actions_contract::JobTimeout;

use crate::{RenderError, workflow_size, yaml::Yaml};

pub(crate) fn validate(path: &str, document: &Yaml) -> Result<(), RenderError> {
    if !workflow_size::is_workflow_path(path) {
        return Ok(());
    }
    for (id, job) in jobs(document, path)? {
        let fields = mapping(job, id)?;
        if local_callee(fields, id)?.is_some() {
            // Reusable callers cannot set timeout-minutes. The product factory
            // separately admits their complete generated inventory below.
            continue;
        }
        let minutes = fields
            .iter()
            .find_map(|(key, value)| (key == "timeout-minutes").then_some(value));
        if let Some(value) = minutes
            && authenticated_preview_timeout(path, id, fields, value)
        {
            continue;
        }
        let Some(Yaml::Int(minutes)) = minutes else {
            return Err(invalid(id, "timeout_missing_or_dynamic"));
        };
        let minutes = u32::try_from(*minutes).map_err(|_| invalid(id, "timeout_invalid"))?;
        JobTimeout::try_from(minutes).map_err(RenderError::Contract)?;
    }
    Ok(())
}

/// Preview runner budgets are produced by the native source admission, which
/// validates the source policy's 1..=30 minute operation limit. Admit only the
/// two exact downstream expressions and their exact selector dependency.
fn authenticated_preview_timeout(
    path: &str,
    id: &str,
    fields: &[(String, Yaml)],
    timeout: &Yaml,
) -> bool {
    if path
        != velnor_actions_contract::package_release::PackageReleaseChannel::Preview.workflow_path()
    {
        return false;
    }
    let expr = match timeout {
        Yaml::Str(value) | Yaml::Quoted(value) => value.as_str(),
        _ => return false,
    };
    let needs = fields
        .iter()
        .find_map(|(key, value)| (key == "needs").then_some(value));
    let Some(Yaml::Seq(needs)) = needs else {
        return false;
    };
    let has_eligibility = needs.iter().any(|need| {
        matches!(need, Yaml::Str(value) | Yaml::Quoted(value) if value == "preview-eligibility")
    });
    if !has_eligibility {
        return false;
    }
    let has_workflow_run_gate = fields.iter().any(|(key, value)| {
        key == "if"
            && matches!(value, Yaml::Str(value) | Yaml::Quoted(value)
                if value == "${{ github.event_name == 'workflow_run' }}")
    });
    if !has_workflow_run_gate {
        return false;
    }
    match (id, expr) {
        ("preview-payload", "${{ matrix.timeout_minutes }}") => {
            let strategy = fields
                .iter()
                .find_map(|(key, value)| (key == "strategy").then_some(value));
            let Some(Yaml::Map(strategy)) = strategy else {
                return false;
            };
            let fail_fast_disabled = strategy.iter().any(|(key, value)| {
                key == "fail-fast" && matches!(value, Yaml::Bool(false))
            });
            fail_fast_disabled
                && strategy.iter().any(|(key, matrix)| {
                    key == "matrix"
                        && matches!(matrix, Yaml::Str(value) | Yaml::Quoted(value)
                            if value == "${{ fromJSON(needs.preview-eligibility.outputs.payload_matrix) }}")
                })
        }
        ("preview-assemble", "${{ needs.preview-eligibility.outputs.producer_timeout_minutes }}") => {
            needs.iter().any(|need| {
                matches!(need, Yaml::Str(value) | Yaml::Quoted(value) if value == "preview-payload")
            })
        }
        _ => false,
    }
}

pub(crate) fn validate_tree(documents: &[(&str, &Yaml)]) -> Result<(), RenderError> {
    let mut paths = BTreeSet::new();
    for (path, document) in documents {
        if !paths.insert(*path) {
            return Err(invalid(path, "duplicate_workflow"));
        }
        validate(path, document)?;
    }
    for (path, document) in documents {
        for (id, job) in jobs(document, path)? {
            if let Some(callee) = local_callee(mapping(job, id)?, id)?
                && !paths.contains(callee)
            {
                return Err(invalid(id, "callee_missing_from_generated_inventory"));
            }
        }
    }
    Ok(())
}

fn local_callee<'a>(
    fields: &'a [(String, Yaml)],
    id: &str,
) -> Result<Option<&'a str>, RenderError> {
    let Some((_, value)) = fields.iter().find(|(key, _)| key == "uses") else {
        return Ok(None);
    };
    let (Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. }) = value else {
        return Err(invalid(id, "dynamic_callee"));
    };
    if fields
        .iter()
        .any(|(key, _)| matches!(key.as_str(), "runs-on" | "steps" | "timeout-minutes"))
    {
        return Err(invalid(id, "mixed_reusable_job"));
    }
    let filename = value
        .strip_prefix("./.github/workflows/")
        .ok_or_else(|| invalid(id, "unbounded_external_callee"))?;
    let stem = filename
        .strip_suffix(".yml")
        .or_else(|| filename.strip_suffix(".yaml"));
    if !stem.is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }) {
        return Err(invalid(id, "noncanonical_callee"));
    }
    Ok(value.strip_prefix("./"))
}

fn jobs<'a>(document: &'a Yaml, path: &str) -> Result<&'a [(String, Yaml)], RenderError> {
    let jobs = mapping(document, path)?
        .iter()
        .find_map(|(key, value)| (key == "jobs").then_some(value))
        .ok_or_else(|| invalid(path, "jobs_missing"))?;
    mapping(jobs, path)
}

fn mapping<'a>(value: &'a Yaml, id: &str) -> Result<&'a [(String, Yaml)], RenderError> {
    match value {
        Yaml::Map(fields)
        | Yaml::AnchoredMap {
            entries: fields, ..
        } => Ok(fields),
        _ => Err(invalid(id, "mapping_required")),
    }
}

fn invalid(id: &str, problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("workflow_timeout:{id}:{problem}"))
}

#[cfg(test)]
#[path = "workflow_timeout_tests.rs"]
mod tests;
