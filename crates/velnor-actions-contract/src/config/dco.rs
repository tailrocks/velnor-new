//! `[dco]`: generated `.github/dco.yml` for the cncf/dco2 GitHub App.
//!
//! Mirrors the documented dco2 reference configuration
//! (<https://github.com/cncf/dco2/blob/main/docs/config/dco.yml>) with
//! `snake_case` keys. Absent `[dco]` emits no file.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Repository-relative path of the generated dco2 configuration.
pub const DCO_CONFIG_PATH: &str = ".github/dco.yml";

/// dco2 configuration (`[dco]` table); every key is optional, at least one is required.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DcoConfig {
    /// `allowOverrideAction`: show the "Set DCO to pass" button on failed checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_override_action: Option<bool>,
    /// `allowRemediationCommits`: retroactive sign-off by remediation commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_remediation_commits: Option<DcoRemediationCommits>,
    /// `require`: who must sign off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require: Option<DcoRequire>,
}

/// `allowRemediationCommits` keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DcoRemediationCommits {
    /// `individual`: the failing commit's author may remediate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub individual: Option<bool>,
    /// `thirdParty`: another person may remediate; needs `individual`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_party: Option<bool>,
}

/// `require` keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DcoRequire {
    /// `members`: organization members must sign off (`false` exempts them).
    pub members: bool,
}

impl DcoConfig {
    /// Validate the table: non-empty, and dco2's remediation dependency.
    /// # Errors
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

#[cfg(test)]
mod tests {
    use super::{DcoConfig, DcoRemediationCommits, DcoRequire};

    const FILE: &str = ".velnor/config.toml";

    fn problem(config: &DcoConfig) -> String {
        config.validate(FILE).expect_err("invalid dco").to_string()
    }

    #[test]
    fn empty_tables_fail_closed() {
        assert!(problem(&DcoConfig::default()).contains("empty_dco_table"));
        let config = DcoConfig {
            allow_remediation_commits: Some(DcoRemediationCommits::default()),
            ..DcoConfig::default()
        };
        assert!(problem(&config).contains("empty_allow_remediation_commits"));
    }

    #[test]
    fn third_party_needs_individual() {
        for individual in [None, Some(false)] {
            let config = DcoConfig {
                allow_remediation_commits: Some(DcoRemediationCommits {
                    individual,
                    third_party: Some(true),
                }),
                ..DcoConfig::default()
            };
            assert!(problem(&config).contains("third_party_requires_individual"));
        }
        let both = DcoConfig {
            allow_remediation_commits: Some(DcoRemediationCommits {
                individual: Some(true),
                third_party: Some(true),
            }),
            ..DcoConfig::default()
        };
        assert!(both.validate(FILE).is_ok());
    }

    #[test]
    fn single_keys_validate() {
        let members = DcoConfig {
            require: Some(DcoRequire { members: false }),
            ..DcoConfig::default()
        };
        assert!(members.validate(FILE).is_ok());
        let override_action = DcoConfig {
            allow_override_action: Some(false),
            ..DcoConfig::default()
        };
        assert!(override_action.validate(FILE).is_ok());
    }
}
