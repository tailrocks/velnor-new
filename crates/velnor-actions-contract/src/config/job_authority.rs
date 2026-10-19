use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// One reviewed role for one workflow job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAuthorityBinding {
    /// Root workflow path.
    pub root_path: String,
    /// Workflow file path that owns the job.
    pub workflow_path: String,
    /// Exact job key.
    pub job_key: String,
    /// Reviewed authority role.
    pub role: JobAuthorityRole,
}

/// Authority granted to one workflow job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobAuthorityRole {
    /// The job verifies source and produces no trusted publication.
    Verification,
    /// The job performs a reviewed trusted publication.
    TrustedPublication,
}

pub(crate) fn validate_job_authority_bindings(
    bindings: &[JobAuthorityBinding],
    file: &str,
) -> Result<(), ContractError> {
    let mut keys = BTreeSet::new();
    for binding in bindings {
        if !workflow_path(&binding.root_path)
            || !workflow_path(&binding.workflow_path)
            || !job_key(&binding.job_key)
        {
            return Err(ContractError::config(
                file,
                "job_authority",
                "invalid_job_binding",
            ));
        }
        let key = (
            binding.root_path.as_str(),
            binding.workflow_path.as_str(),
            binding.job_key.as_str(),
        );
        if !keys.insert(key) {
            return Err(ContractError::config(
                file,
                "job_authority",
                "duplicate_job_binding",
            ));
        }
    }
    Ok(())
}

fn workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && !path.contains("..")
        && !path.contains('\\')
        && path.bytes().all(|byte| byte.is_ascii_graphic())
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
}

fn job_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(job_key: &str) -> JobAuthorityBinding {
        JobAuthorityBinding {
            root_path: ".github/workflows/ci.yml".to_owned(),
            workflow_path: ".github/workflows/ci.yml".to_owned(),
            job_key: job_key.to_owned(),
            role: JobAuthorityRole::Verification,
        }
    }

    #[test]
    fn accepts_unique_workflow_job_binding() {
        assert!(validate_job_authority_bindings(&[binding("test")], ".velnor/config.toml").is_ok());
    }

    #[test]
    fn rejects_duplicate_workflow_job_binding() {
        let error = validate_job_authority_bindings(
            &[binding("test"), binding("test")],
            ".velnor/config.toml",
        )
        .expect_err("duplicate binding must fail");
        assert!(error.to_string().contains("duplicate_job_binding"));
    }

    #[test]
    fn rejects_invalid_workflow_path() {
        let mut value = binding("test");
        value.workflow_path = ".github/workflows/../ci.yml".to_owned();
        assert!(validate_job_authority_bindings(&[value], ".velnor/config.toml").is_err());
    }
}
