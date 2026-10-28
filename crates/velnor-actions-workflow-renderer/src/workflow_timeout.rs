// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::JobTimeout;

use crate::{RenderError, workflow_size, yaml::Yaml};

pub(super) struct HomebrewAdoptionCalleeScope {
    source_sha: String,
    transition_id: String,
    sdk_version: String,
    manifest_sha256: String,
    callee_sha256: String,
}

impl HomebrewAdoptionCalleeScope {
    pub(super) fn authenticated_seed(
        source_sha: &str,
        transition_id: &str,
        sdk_version: &str,
        manifest_sha256: &str,
        callee_sha256: &str,
    ) -> Self {
        Self {
            source_sha: source_sha.to_owned(),
            transition_id: transition_id.to_owned(),
            sdk_version: sdk_version.to_owned(),
            manifest_sha256: manifest_sha256.to_owned(),
            callee_sha256: callee_sha256.to_owned(),
        }
    }

    fn callee_ref(&self) -> String {
        format!(
            "tailrocks/velnor-new/.github/workflows/homebrew-adoption.yml@{}",
            self.source_sha
        )
    }
}

pub(super) fn validate_homebrew_adoption_caller(
    path: &str,
    document: &Yaml,
    scope: &HomebrewAdoptionCalleeScope,
) -> Result<(), RenderError> {
    if path != ".github/workflows/ci.yml" || !lower_hex(&scope.callee_sha256, 64) {
        return Err(invalid(path, "seed_caller_path"));
    }
    let top = mapping(document, path)?;
    if top.len() != 4
        || !top
            .iter()
            .any(|(key, value)| key == "name" && value == &Yaml::str("CI"))
        || !top
            .iter()
            .any(|(key, value)| key == "on" && *value == pull_request_only())
        || !top
            .iter()
            .any(|(key, value)| key == "permissions" && *value == caller_permissions())
    {
        return Err(invalid(path, "seed_caller_inventory"));
    }
    let jobs = jobs(document, path)?;
    if jobs.len() != 1 || jobs[0].0 != "homebrew-adoption" {
        return Err(invalid(path, "seed_caller_jobs"));
    }
    let job = mapping(&jobs[0].1, "homebrew-adoption")?;
    let expected_with = Yaml::Map(vec![
        (
            "transition_id".to_owned(),
            Yaml::str(scope.transition_id.clone()),
        ),
        (
            "sdk_version".to_owned(),
            Yaml::str(scope.sdk_version.clone()),
        ),
        (
            "sdk_source_sha".to_owned(),
            Yaml::str(scope.source_sha.clone()),
        ),
        (
            "sdk_manifest_sha256".to_owned(),
            Yaml::str(scope.manifest_sha256.clone()),
        ),
    ]);
    let expected_job_permissions = Yaml::Map(vec![
        ("contents".to_owned(), Yaml::str("read")),
        ("actions".to_owned(), Yaml::str("read")),
        ("attestations".to_owned(), Yaml::str("read")),
    ]);
    if job.len() != 3
        || !job
            .iter()
            .any(|(key, value)| key == "uses" && value == &Yaml::str(scope.callee_ref()))
        || !job
            .iter()
            .any(|(key, value)| key == "with" && value == &expected_with)
        || !job
            .iter()
            .any(|(key, value)| key == "permissions" && value == &expected_job_permissions)
    {
        return Err(invalid("homebrew-adoption", "seed_callee_scope"));
    }
    Ok(())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn pull_request_only() -> Yaml {
    Yaml::Map(vec![(
        "pull_request".to_owned(),
        Yaml::Map(vec![(
            "types".to_owned(),
            Yaml::Seq(
                crate::render::EXPECTED_PR_TYPES
                    .iter()
                    .copied()
                    .map(Yaml::str)
                    .collect(),
            ),
        )]),
    )])
}

fn caller_permissions() -> Yaml {
    Yaml::Map(vec![
        ("contents".to_owned(), Yaml::str("read")),
        ("actions".to_owned(), Yaml::str("read")),
        ("attestations".to_owned(), Yaml::str("read")),
    ])
}

pub(crate) fn validate(path: &str, document: &Yaml) -> Result<(), RenderError> {
    if !workflow_size::is_workflow_path(path) {
        return Ok(());
    }
    for (id, job) in jobs(document, path)? {
        let fields = mapping(job, id)?;
        if local_callee(fields, id)?.is_some() {
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
        (
            "preview-assemble",
            "${{ fromJSON(needs.preview-eligibility.outputs.producer_timeout_minutes) }}",
        ) => {
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
