use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

pub const DCO_CONFIG_PATH: &str = ".github/dco.yml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DcoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_override_action: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_remediation_commits: Option<DcoRemediationCommits>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require: Option<DcoRequire>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DcoRemediationCommits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub individual: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_party: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DcoRequire {
    pub members: bool,
}

impl DcoConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.allow_override_action.is_none()
            && self.allow_remediation_commits.is_none()
            && self.require.is_none()
        {
            return Err(ContractError::config(file, "dco", "empty_dco_table"));
        }
        if let Some(remediation) = &self.allow_remediation_commits {
            if remediation.individual.is_none() && remediation.third_party.is_none() {
                return Err(ContractError::config(
                    file,
                    "dco.allow_remediation_commits",
                    "empty_allow_remediation_commits",
                ));
            }
            if remediation.third_party == Some(true) && remediation.individual != Some(true) {
                return Err(ContractError::config(
                    file,
                    "dco.allow_remediation_commits.third_party",
                    "third_party_requires_individual",
                ));
            }
        }
        Ok(())
    }
}
