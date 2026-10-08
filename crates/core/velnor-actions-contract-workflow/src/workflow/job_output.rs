//! Typed job outputs bound to one validated workflow source.

use super::{
    step::Step,
    step_identity::{StepId, StepRole},
};
use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

/// Supported workflow job-output names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOutputName {
    /// Numeric ID emitted by the pinned task-report artifact upload.
    TaskReportArtifactId,
    /// Numeric ID exposed by the current GitHub Actions job context.
    TaskReportCheckRunId,
}

impl JobOutputName {
    /// Exact YAML job-output key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TaskReportArtifactId => "task_report_artifact_id",
            Self::TaskReportCheckRunId => "task_report_check_run_id",
        }
    }
}

/// Supported output exposed by a typed workflow step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepOutputName {
    /// Numeric artifact ID emitted by `actions/upload-artifact`.
    ArtifactId,
}

impl StepOutputName {
    /// Exact GitHub Actions output key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArtifactId => "artifact-id",
        }
    }
}

/// One job output whose value comes from a typed step or job-context source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobOutput {
    /// Job-level output name available through direct dependents' `needs`.
    pub name: JobOutputName,
    /// Exact typed source that produces this value.
    pub source: JobOutputSource,
}

/// Typed source of one job output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JobOutputSource {
    /// Exact output of a stable workflow step identity.
    Step {
        /// Stable workflow step identity.
        step: StepId,
        /// Output key declared by that step's action contract.
        output: StepOutputName,
    },
    /// Exact field of the GitHub Actions `job` context.
    JobContext {
        /// Supported job-context output field.
        context: JobOutputContext,
    },
}

/// Supported GitHub Actions job-context output fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOutputContext {
    /// `job.check_run_id`, available on github.com-hosted Actions.
    CheckRunId,
}

impl JobOutput {
    /// Expose the report-upload action's actual artifact ID to dependents.
    #[must_use]
    pub const fn task_report_artifact_id() -> Self {
        Self {
            name: JobOutputName::TaskReportArtifactId,
            source: JobOutputSource::Step {
                step: StepId::CrateReportUpload,
                output: StepOutputName::ArtifactId,
            },
        }
    }

    /// Expose the current job's GitHub Check Run ID to direct dependents.
    #[must_use]
    pub const fn task_report_check_run_id() -> Self {
        Self {
            name: JobOutputName::TaskReportCheckRunId,
            source: JobOutputSource::JobContext {
                context: JobOutputContext::CheckRunId,
            },
        }
    }

    /// Render the one validated source expression used in workflow YAML.
    #[must_use]
    pub fn expression(&self) -> String {
        match self.source {
            JobOutputSource::Step { step, output } => {
                format!(
                    "${{{{ steps.{}.outputs.{} }}}}",
                    step.as_str(),
                    output.as_str()
                )
            }
            JobOutputSource::JobContext {
                context: JobOutputContext::CheckRunId,
            } => "${{ job.check_run_id }}".to_owned(),
        }
    }
}

/// Validate report artifact outputs against their unique typed upload step.
pub(crate) fn validate_job_outputs(
    outputs: &[JobOutput],
    steps: &[Step],
    job_id: &str,
) -> Result<(), ContractError> {
    let report_uploads: Vec<&Step> = steps
        .iter()
        .filter(|step| step.role == Some(StepRole::CrateReportUpload))
        .collect();
    if report_uploads.len() > 1 {
        return Err(invalid(job_id, "report_upload_count"));
    }
    if report_uploads.is_empty() && outputs.is_empty() {
        return Ok(());
    }
    if report_uploads.len() != 1 || outputs.is_empty() || outputs.len() > 2 {
        return Err(invalid(job_id, "report_output_count"));
    }
    let artifact_outputs: Vec<&JobOutput> = outputs
        .iter()
        .filter(|output| output.name == JobOutputName::TaskReportArtifactId)
        .collect();
    let check_run_outputs: Vec<&JobOutput> = outputs
        .iter()
        .filter(|output| output.name == JobOutputName::TaskReportCheckRunId)
        .collect();
    if artifact_outputs.len() != 1 || check_run_outputs.len() > 1 {
        return Err(invalid(job_id, "report_output_count"));
    }
    let expected_artifact = JobOutput::task_report_artifact_id();
    if artifact_outputs[0] != &expected_artifact {
        return Err(invalid(job_id, "report_output_source_mismatch"));
    }
    if check_run_outputs
        .first()
        .is_some_and(|output| **output != JobOutput::task_report_check_run_id())
    {
        return Err(invalid(job_id, "report_check_run_output_source_mismatch"));
    }
    let source_exists = steps.iter().any(|step| {
        step.id == Some(StepId::CrateReportUpload) && step.role == Some(StepRole::CrateReportUpload)
    });
    if !source_exists {
        return Err(invalid(job_id, "report_output_step_missing"));
    }
    Ok(())
}

fn invalid(job_id: &str, reason: &str) -> ContractError {
    ContractError::identity("job.outputs", format!("{reason}:{job_id}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::workflow::step::StepKind;

    use super::*;

    fn report_upload() -> Step {
        Step {
            name: "Upload report".to_owned(),
            id: Some(StepId::CrateReportUpload),
            role: Some(StepRole::CrateReportUpload),
            condition: None,
            kind: StepKind::Action {
                uses: "actions/upload-artifact@v4".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn empty_job_outputs_are_omitted_and_legacy_jobs_deserialize() {
        let job = crate::workflow::Job {
            display_name: "Lint".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: crate::workflow::JobTimeout::CRATE,
            outputs: Vec::new(),
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: Vec::new(),
        };
        let value = serde_json::to_value(job).expect("workflow job serializes");
        assert!(value.get("outputs").is_none());
        let legacy: crate::workflow::Job =
            serde_json::from_value(value).expect("legacy job without outputs deserializes");
        assert_eq!(legacy.outputs, Vec::<JobOutput>::new());
    }

    #[test]
    fn task_report_output_is_exactly_bound_to_upload_artifact_id() {
        let output = JobOutput::task_report_artifact_id();
        assert_eq!(output.name.as_str(), "task_report_artifact_id");
        assert_eq!(
            output.expression(),
            "${{ steps.crate-report-upload.outputs.artifact-id }}"
        );
    }

    #[test]
    fn check_run_output_is_a_typed_job_context_source() {
        let output = JobOutput::task_report_check_run_id();
        assert_eq!(output.name.as_str(), "task_report_check_run_id");
        assert_eq!(output.expression(), "${{ job.check_run_id }}");
        assert_eq!(
            serde_json::to_value(output).expect("typed output serializes"),
            serde_json::json!({
                "name": "task_report_check_run_id",
                "source": { "context": "check_run_id" }
            })
        );
    }

    #[test]
    fn legacy_artifact_only_output_shape_still_deserializes_and_validates() {
        let legacy = serde_json::json!({
            "name": "task_report_artifact_id",
            "source": {
                "step": "crate-report-upload",
                "output": "artifact-id"
            }
        });
        let output: JobOutput =
            serde_json::from_value(legacy.clone()).expect("legacy output deserializes");
        assert_eq!(
            serde_json::to_value(&output).expect("legacy output serializes"),
            legacy
        );
        assert!(validate_job_outputs(&[output], &[report_upload()], "legacy-task").is_ok());
    }

    #[test]
    fn job_output_requires_one_matching_upload_step() {
        let output = JobOutput::task_report_artifact_id();
        let step = report_upload();
        assert!(
            validate_job_outputs(
                std::slice::from_ref(&output),
                std::slice::from_ref(&step),
                "rust-demo"
            )
            .is_ok()
        );
        let check_run = JobOutput::task_report_check_run_id();
        assert!(
            validate_job_outputs(
                &[output.clone(), check_run.clone()],
                std::slice::from_ref(&step),
                "rust-demo"
            )
            .is_ok()
        );
        assert!(validate_job_outputs(&[], &[], "lint").is_ok());
        assert!(validate_job_outputs(std::slice::from_ref(&output), &[], "rust-demo").is_err());
        assert!(
            validate_job_outputs(
                std::slice::from_ref(&check_run),
                &[step.clone()],
                "rust-demo"
            )
            .is_err()
        );
        assert!(validate_job_outputs(&[], std::slice::from_ref(&step), "rust-demo").is_err());
        assert!(
            validate_job_outputs(
                &[output.clone(), output.clone()],
                &[step.clone()],
                "rust-demo"
            )
            .is_err()
        );
        assert!(
            validate_job_outputs(
                &[check_run.clone(), check_run],
                &[step.clone()],
                "rust-demo"
            )
            .is_err()
        );
        assert!(
            validate_job_outputs(
                std::slice::from_ref(&output),
                &[step.clone(), step.clone()],
                "rust-demo"
            )
            .is_err()
        );

        let mut wrong_source = output;
        wrong_source.source = JobOutputSource::Step {
            step: StepId::Plan,
            output: StepOutputName::ArtifactId,
        };
        assert!(validate_job_outputs(&[wrong_source], &[step], "rust-demo").is_err());

        let wrong_context = JobOutput {
            name: JobOutputName::TaskReportCheckRunId,
            source: JobOutputSource::Step {
                step: StepId::CrateReportUpload,
                output: StepOutputName::ArtifactId,
            },
        };
        assert!(
            validate_job_outputs(
                &[JobOutput::task_report_artifact_id(), wrong_context],
                &[report_upload()],
                "rust-demo"
            )
            .is_err()
        );
    }
}
