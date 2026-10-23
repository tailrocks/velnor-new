//! Validation of generator-resolved immutable collector specifications.

use super::{
    GITHUB_EVIDENCE_SNAPSHOT_SCHEMA, GitHubEvidenceFetchSpecV1, GitHubEvidenceWorkflowSourceRefV1,
};

impl GitHubEvidenceFetchSpecV1 {
    /// Parse strict canonical JSON into a validated generated request.
    /// # Errors
    pub fn parse_canonical_json(text: &str) -> Result<Self, String> {
        let value = crate::strict_json::parse_strict_json(text)
            .map_err(|_| "github_evidence_fetch_spec_json".to_owned())?;
        let spec: Self = serde_json::from_value(value)
            .map_err(|_| "github_evidence_fetch_spec_shape".to_owned())?;
        if crate::canonical_json_bytes(&spec)
            .map_err(|_| "github_evidence_fetch_spec_canonical".to_owned())?
            != text.as_bytes()
        {
            return Err("github_evidence_fetch_spec_noncanonical".to_owned());
        }
        spec.validate()?;
        Ok(spec)
    }

    /// Validate the fully resolved closed request before serialization/use.
    /// # Errors
    pub fn validate(&self) -> Result<(), String> {
        validate_header(self)?;
        validate_workflow_sources(self)?;
        validate_artifact_sources(self)
    }
}

fn validate_header(spec: &GitHubEvidenceFetchSpecV1) -> Result<(), String> {
    if spec.schema != GITHUB_EVIDENCE_SNAPSHOT_SCHEMA
        || !crate::is_valid_workflow_task_id(&spec.collector_suite_id)
        || !spec
            .collector_workflow_path
            .strip_prefix(".github/workflows/")
            .and_then(|path| path.strip_suffix(".yml"))
            .is_some_and(valid_workflow_slug)
        || !(1..=31).contains(&spec.window_days)
        || !crate::is_valid_branch_name(&spec.default_branch)
    {
        return Err("github_evidence_fetch_spec_header".to_owned());
    }
    Ok(())
}

fn validate_workflow_sources(spec: &GitHubEvidenceFetchSpecV1) -> Result<(), String> {
    if spec.workflow_sources.is_empty()
        || spec.workflow_sources.len() > 16
        || !spec
            .workflow_sources
            .iter()
            .any(|source| source.source == GitHubEvidenceWorkflowSourceRefV1::PrimaryCi)
        || spec
            .workflow_sources
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err("github_evidence_fetch_spec_workflows".to_owned());
    }
    let mut workflow_paths = std::collections::BTreeSet::new();
    for source in &spec.workflow_sources {
        let valid = match &source.source {
            GitHubEvidenceWorkflowSourceRefV1::PrimaryCi => {
                source.path == ".github/workflows/ci.yml"
                    && source.job_id.is_none()
                    && source.job_name.is_none()
            }
            GitHubEvidenceWorkflowSourceRefV1::TaskSuite { suite_id } => {
                let expected_job = format!("mise-task-{suite_id}");
                let expected_name = format!("Mise task {suite_id}");
                let slug = source
                    .path
                    .strip_prefix(".github/workflows/")
                    .and_then(|path| path.strip_suffix(".yml"));
                crate::is_valid_workflow_task_id(suite_id)
                    && slug.is_some_and(valid_workflow_slug)
                    && source.job_id.as_deref() == Some(expected_job.as_str())
                    && source.job_name.as_deref() == Some(expected_name.as_str())
            }
        };
        if !valid || !workflow_paths.insert(source.path.as_str()) {
            return Err("github_evidence_fetch_spec_workflow_identity".to_owned());
        }
    }
    Ok(())
}

fn validate_artifact_sources(spec: &GitHubEvidenceFetchSpecV1) -> Result<(), String> {
    if spec.artifact_sources.is_empty()
        || spec.artifact_sources.len() > 64
        || spec
            .artifact_sources
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err("github_evidence_fetch_spec_artifacts".to_owned());
    }
    for artifact in &spec.artifact_sources {
        let expected_path = spec
            .workflow_sources
            .iter()
            .find_map(|source| match &source.source {
                GitHubEvidenceWorkflowSourceRefV1::TaskSuite { suite_id }
                    if suite_id == &artifact.suite_id =>
                {
                    Some(source.path.as_str())
                }
                _ => None,
            });
        if artifact.suite_id.is_empty()
            || !crate::is_valid_workflow_task_id(&artifact.suite_id)
            || !valid_evidence_artifact_name(&artifact.artifact_name)
            || artifact.job_id != format!("mise-task-{}", artifact.suite_id)
            || artifact.job_name != format!("Mise task {}", artifact.suite_id)
            || expected_path != Some(artifact.workflow_path.as_str())
        {
            return Err("github_evidence_fetch_spec_artifact_identity".to_owned());
        }
    }
    Ok(())
}

fn valid_workflow_slug(slug: &str) -> bool {
    let mut bytes = slug.bytes();
    matches!(bytes.next(), Some(byte) if byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn valid_evidence_artifact_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
