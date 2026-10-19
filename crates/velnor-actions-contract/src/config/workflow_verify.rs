use super::VerifyConfig;
use crate::errors::ContractError;
use crate::workflow::ValidatorKind;

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
                if !valid_json_path(path) || !paths.insert(path) {
                    return Err(ContractError::config(
                        file,
                        "workflow.verify.strict_json_files",
                        format!("invalid_or_duplicate_path:{path}"),
                    ));
                }
            }
        }
        for arg in &self.lychee_args {
            if !valid_lychee_arg(arg) {
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

fn valid_json_path(value: &str) -> bool {
    !value.is_empty()
        && std::path::Path::new(value)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        && !value.starts_with('/')
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn valid_lychee_arg(value: &str) -> bool {
    !value.is_empty()
        && value != "--"
        && !value.contains("${{")
        && !value.contains(['$', '`'])
        && !value.chars().any(char::is_control)
        && !value.split_whitespace().any(|word| word == "mise")
}
