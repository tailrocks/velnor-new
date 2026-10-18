use crate::errors::ContractError;

pub(super) fn is_source_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn missing(key: &str) -> ContractError {
    ContractError::config("config.toml", key, "missing")
}

use super::{ExecutionProfile, LINUX_AMD64, ProfileKind, RUNNER_LABEL_CATALOG, ScaleSetSelector};
use crate::config::runs_on::is_hosted_catalog;

impl ExecutionProfile {
    /// Hosted catalog profile.
    ///
    /// # Errors
    ///
    /// The label must be in the hosted catalog.
    pub fn hosted(label: &str) -> Result<Self, ContractError> {
        if !RUNNER_LABEL_CATALOG.contains(&label) || !is_hosted_catalog(label) {
            return Err(ContractError::config(
                "config.toml",
                "execution.profiles.hosted.label",
                format!("unsupported_label:{label}"),
            ));
        }
        Ok(Self {
            kind: ProfileKind::GithubHosted,
            label: Some(label.to_owned()),
            name: None,
            labels: Vec::new(),
            platform: LINUX_AMD64.to_owned(),
            capability_profile: None,
            image_source_sha: None,
            job_container_image: None,
        })
    }

    /// Scale-set profile from an already validated selector.
    #[must_use]
    pub fn scale_set(selector: &ScaleSetSelector) -> Self {
        Self {
            kind: ProfileKind::GithubScaleSet,
            label: None,
            name: Some(selector.name().to_owned()),
            labels: selector.labels().to_vec(),
            platform: LINUX_AMD64.to_owned(),
            capability_profile: None,
            image_source_sha: None,
            job_container_image: None,
        }
    }
}
