//! Opaque credential transport validation and the explicit GitHub-token policy.

use std::ffi::OsString;

use crate::error::MiseError;

use super::{EnvPolicy, IsolatedCommand};

/// True when a value is safe to transport as an opaque credential.
///
/// This checks only the byte set needed by Velnor's environment and config
/// transports. It does not authenticate the value or assume a token format.
#[must_use]
pub fn is_safe_credential_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

impl IsolatedCommand {
    pub(crate) fn with_github_token(mut self, token: &str) -> Result<Self, MiseError> {
        if !is_safe_credential_value(token) {
            return Err(MiseError::InvalidStepInput {
                field: "GH_TOKEN".to_owned(),
                value: "credential_rejected".to_owned(),
            });
        }
        self.extra_env
            .push((OsString::from("GH_TOKEN"), OsString::from(token)));
        self.policy = EnvPolicy::GitHubToken;
        Ok(self)
    }
}
