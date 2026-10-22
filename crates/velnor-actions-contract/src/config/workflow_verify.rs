//! Validation for `[workflow.verify]` configuration.

use crate::errors::ContractError;
use crate::workflow::ValidatorKind;

use super::VerifyConfig;

/// The retired zizmor job (hard decision 10): jactionlint's `default` profile
/// runs its audits, so naming it is an error that says what to set instead.
const RETIRED_ZIZMOR_JOB: &str = "zizmor";

impl VerifyConfig {
    /// True when no verification job is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Validate the verify allowlist: known IDs, no duplicates.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = std::collections::BTreeSet::new();
        for job in &self.jobs {
            if job == RETIRED_ZIZMOR_JOB {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    "zizmor_retired:jactionlint covers the zizmor audits; set [workflow.lint] profile = \"default\" and remove \"zizmor\" (hard decision 10)".to_owned(),
                ));
            }
            if ValidatorKind::from_verify_name(job).is_none() {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    format!("unknown_verify_job:{job}"),
                ));
            }
            if !seen.insert(job) {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.jobs",
                    format!("duplicate_verify_job:{job}"),
                ));
            }
        }
        if let Some(files) = &self.strict_json_files {
            if !self.jobs.iter().any(|job| job == "strict-json") || files.is_empty() {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.strict_json_files",
                    "requires_strict_json_and_nonempty_files",
                ));
            }
            let mut paths = std::collections::BTreeSet::new();
            for path in files {
                if !super::workflow_validation_helpers::valid_json_path(path) || !paths.insert(path)
                {
                    return Err(ContractError::config(
                        file,
                        "workflow.verify.strict_json_files",
                        format!("invalid_or_duplicate_path:{path}"),
                    ));
                }
            }
        }
        for arg in &self.lychee_args {
            if !super::workflow_validation_helpers::valid_lychee_arg(arg) {
                return Err(ContractError::config(
                    file,
                    "workflow.verify.lychee_args",
                    format!("invalid_or_duplicate_lychee_arg:{arg}"),
                ));
            }
        }
        if !self.lychee_args.is_empty() && !self.jobs.iter().any(|job| job == "link-check") {
            return Err(ContractError::config(
                file,
                "workflow.verify.lychee_args",
                "requires_link_check",
            ));
        }
        Ok(())
    }
}
