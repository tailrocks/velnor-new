// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::error::MiseError;

use super::{EnvPolicy, IsolatedCommand};

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
