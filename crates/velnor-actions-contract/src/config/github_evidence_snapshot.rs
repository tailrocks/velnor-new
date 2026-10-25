use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{MiseTaskSuite, MiseTaskSuiteEvent};
use crate::{
    ContractError, GITHUB_EVIDENCE_SNAPSHOT_SCHEMA, GitHubEvidenceArtifactRefV1,
    GitHubEvidenceFetchSpecV1, GitHubEvidenceWorkflowRefV1, GitHubEvidenceWorkflowSourceRefV1,
};

pub const MAX_GITHUB_EVIDENCE_WINDOW_DAYS: u8 = 31;

pub const MAX_GITHUB_EVIDENCE_WORKFLOWS: usize = 16;

pub const MAX_GITHUB_EVIDENCE_ARTIFACTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceSnapshotInput {
    pub window_days: u8,

    pub workflow_sources: Vec<GitHubEvidenceWorkflowSource>,

    pub artifact_sources: Vec<GitHubEvidenceArtifactSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitHubEvidenceWorkflowSource {
    PrimaryCi,

    TaskSuite { suite_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubEvidenceArtifactSource {
    pub suite_id: String,

    pub artifact_name: String,
}

impl GitHubEvidenceSnapshotInput {
    pub fn validate(&self, file: &str, prefix: &str) -> Result<(), ContractError> {
        if !(1..=MAX_GITHUB_EVIDENCE_WINDOW_DAYS).contains(&self.window_days) {
            return Err(ContractError::config(
                file,
                format!("{prefix}.window_days"),
                format!("github_evidence_window_out_of_range:{}", self.window_days),
            ));
        }
        if self.workflow_sources.is_empty()
            || self.workflow_sources.len() > MAX_GITHUB_EVIDENCE_WORKFLOWS
            || !self
                .workflow_sources
                .contains(&GitHubEvidenceWorkflowSource::PrimaryCi)
            || self
                .workflow_sources
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(ContractError::config(
                file,
                format!("{prefix}.workflow_sources"),
                "github_evidence_workflows_must_be_sorted_unique_and_include_primary_ci",
            ));
        }
        if self.artifact_sources.is_empty()
            || self.artifact_sources.len() > MAX_GITHUB_EVIDENCE_ARTIFACTS
            || self
                .artifact_sources
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(ContractError::config(
                file,
                format!("{prefix}.artifact_sources"),
                "github_evidence_artifacts_must_be_nonempty_sorted_unique_and_bounded",
            ));
        }
        Ok(())
    }

    pub fn validate_inventory(
        &self,
        suites: &[MiseTaskSuite],
        collector_id: &str,
        file: &str,
        prefix: &str,
    ) -> Result<(), ContractError> {
        self.validate(file, prefix)?;
        let source_suite_ids = self.validate_workflow_inventory(suites, file, prefix)?;
        self.validate_artifact_inventory(suites, collector_id, &source_suite_ids, file, prefix)
    }

    fn validate_workflow_inventory(
        &self,
        suites: &[MiseTaskSuite],
        file: &str,
        prefix: &str,
    ) -> Result<BTreeSet<String>, ContractError> {
        let source_suite_ids = self
            .workflow_sources
            .iter()
            .filter_map(|source| match source {
                GitHubEvidenceWorkflowSource::PrimaryCi => None,
                GitHubEvidenceWorkflowSource::TaskSuite { suite_id } => Some(suite_id.clone()),
            })
            .collect::<BTreeSet<_>>();
        for suite_id in &source_suite_ids {
            let Some(source) = suites.iter().find(|suite| suite.id == *suite_id) else {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.workflow_sources"),
                    format!("github_evidence_unknown_task_suite:{suite_id}"),
                ));
            };
            if !source.events.contains(&MiseTaskSuiteEvent::Push) {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.workflow_sources"),
                    format!("github_evidence_source_not_push_enabled:{suite_id}"),
                ));
            }
        }
        Ok(source_suite_ids)
    }

    fn validate_artifact_inventory(
        &self,
        suites: &[MiseTaskSuite],
        collector_id: &str,
        source_suite_ids: &BTreeSet<String>,
        file: &str,
        prefix: &str,
    ) -> Result<(), ContractError> {
        for artifact in &self.artifact_sources {
            if artifact.suite_id == collector_id {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.artifact_sources"),
                    "github_evidence_collector_cannot_consume_own_artifact",
                ));
            }
            if !source_suite_ids.contains(artifact.suite_id.as_str()) {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.artifact_sources"),
                    format!(
                        "github_evidence_artifact_workflow_source_missing:{}",
                        artifact.suite_id
                    ),
                ));
            }
            let Some(source) = suites.iter().find(|suite| suite.id == artifact.suite_id) else {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.artifact_sources"),
                    format!(
                        "github_evidence_unknown_artifact_suite:{}",
                        artifact.suite_id
                    ),
                ));
            };
            let Some(declaration) = source
                .artifacts
                .iter()
                .find(|candidate| candidate.name == artifact.artifact_name)
            else {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.artifact_sources"),
                    format!(
                        "github_evidence_unknown_artifact:{}:{}",
                        artifact.suite_id, artifact.artifact_name
                    ),
                ));
            };
            let minimum_retention = u16::from(self.window_days).saturating_add(2);
            if declaration.when != super::RustSuiteArtifactWhen::Always
                || declaration.retention_days < minimum_retention
            {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.artifact_sources"),
                    format!(
                        "github_evidence_artifact_must_be_always_and_retained:{}:{}:{}",
                        artifact.suite_id, artifact.artifact_name, minimum_retention
                    ),
                ));
            }
        }
        Ok(())
    }

    pub fn resolve_spec(
        &self,
        suites: &[MiseTaskSuite],
        collector_suite_id: &str,
        default_branch: &str,
    ) -> Result<GitHubEvidenceFetchSpecV1, ContractError> {
        let collector_suite = suites
            .iter()
            .find(|suite| suite.id == collector_suite_id)
            .ok_or_else(|| {
                ContractError::config(
                    ".velnor/config.toml",
                    "workflow.task_suites.evidence_snapshot",
                    format!("github_evidence_unknown_collector_suite:{collector_suite_id}"),
                )
            })?;
        let workflow_sources = self.resolve_workflow_sources(suites)?;
        let artifact_sources = self.resolve_artifact_sources(suites)?;
        let spec = GitHubEvidenceFetchSpecV1 {
            schema: GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
            collector_suite_id: collector_suite_id.to_owned(),
            collector_workflow_path: format!(".github/workflows/{}.yml", collector_suite.workflow),
            default_branch: default_branch.to_owned(),
            window_days: self.window_days,
            workflow_sources,
            artifact_sources,
        };
        spec.validate().map_err(|problem| {
            ContractError::config(
                ".velnor/config.toml",
                "workflow.task_suites.evidence_snapshot",
                problem,
            )
        })?;
        Ok(spec)
    }

    fn resolve_workflow_sources(
        &self,
        suites: &[MiseTaskSuite],
    ) -> Result<Vec<GitHubEvidenceWorkflowRefV1>, ContractError> {
        self.workflow_sources
            .iter()
            .map(|source| match source {
                GitHubEvidenceWorkflowSource::PrimaryCi => Ok(GitHubEvidenceWorkflowRefV1 {
                    source: GitHubEvidenceWorkflowSourceRefV1::PrimaryCi,
                    path: ".github/workflows/ci.yml".to_owned(),
                    job_id: None,
                    job_name: None,
                }),
                GitHubEvidenceWorkflowSource::TaskSuite { suite_id } => {
                    let suite = suites
                        .iter()
                        .find(|suite| suite.id == *suite_id)
                        .ok_or_else(|| {
                            ContractError::config(
                                ".velnor/config.toml",
                                "workflow.task_suites.evidence_snapshot.workflow_sources",
                                format!("github_evidence_unknown_task_suite:{suite_id}"),
                            )
                        })?;
                    Ok(GitHubEvidenceWorkflowRefV1 {
                        source: GitHubEvidenceWorkflowSourceRefV1::TaskSuite {
                            suite_id: suite_id.clone(),
                        },
                        path: format!(".github/workflows/{}.yml", suite.workflow),
                        job_id: Some(format!("mise-task-{suite_id}")),
                        job_name: Some(format!("Mise task {suite_id}")),
                    })
                }
            })
            .collect()
    }

    fn resolve_artifact_sources(
        &self,
        suites: &[MiseTaskSuite],
    ) -> Result<Vec<GitHubEvidenceArtifactRefV1>, ContractError> {
        let mut artifact_sources = Vec::with_capacity(self.artifact_sources.len());
        for artifact in &self.artifact_sources {
            let suite = suites
                .iter()
                .find(|suite| suite.id == artifact.suite_id)
                .ok_or_else(|| {
                    ContractError::config(
                        ".velnor/config.toml",
                        "workflow.task_suites.evidence_snapshot.artifact_sources",
                        format!(
                            "github_evidence_unknown_artifact_suite:{}",
                            artifact.suite_id
                        ),
                    )
                })?;
            artifact_sources.push(GitHubEvidenceArtifactRefV1 {
                suite_id: artifact.suite_id.clone(),
                artifact_name: artifact.artifact_name.clone(),
                workflow_path: format!(".github/workflows/{}.yml", suite.workflow),
                job_id: format!("mise-task-{}", artifact.suite_id),
                job_name: format!("Mise task {}", artifact.suite_id),
            });
        }
        Ok(artifact_sources)
    }
}
