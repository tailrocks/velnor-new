use std::ffi::OsString;

use super::{EnvPolicy, IsolatedCommand};
use crate::MiseError;

impl IsolatedCommand {
    pub(crate) fn with_github_actions_oidc(self) -> Result<Self, MiseError> {
        if self.policy != EnvPolicy::ReleaseSigner {
            return Err(MiseError::InvalidStepInput {
                field: "release_signer_environment".to_owned(),
                value: "policy_mismatch".to_owned(),
            });
        }
        let values = [
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        ]
        .map(|name| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| MiseError::InvalidStepInput {
                    field: name.to_owned(),
                    value: "required".to_owned(),
                })
        });
        let [url, token] = values;
        self.with_github_actions_oidc_values(url?, token?)
    }

    pub(super) fn with_github_actions_oidc_values(
        mut self,
        url: OsString,
        token: OsString,
    ) -> Result<Self, MiseError> {
        if self.policy != EnvPolicy::ReleaseSigner {
            return Err(MiseError::InvalidStepInput {
                field: "release_signer_environment".to_owned(),
                value: "policy_mismatch".to_owned(),
            });
        }
        if url.is_empty() || token.is_empty() {
            return Err(MiseError::InvalidStepInput {
                field: "actions_oidc".to_owned(),
                value: "required".to_owned(),
            });
        }
        self.extra_env
            .push((OsString::from("ACTIONS_ID_TOKEN_REQUEST_URL"), url));
        self.extra_env
            .push((OsString::from("ACTIONS_ID_TOKEN_REQUEST_TOKEN"), token));
        Ok(self)
    }
}
