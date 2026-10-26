// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::{RenderError, steps::INTERNAL_OP_ENV, yaml::Yaml};

pub(super) fn internal_env(
    operation: &str,
    step_env: &BTreeMap<String, String>,
) -> Result<Option<Yaml>, RenderError> {
    let (token_key, token_value, expected_len, error) = match operation {
        velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION => (
            Some(velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV),
            Some("${{ github.token }}"),
            2,
            "github_evidence_fetch_env_shape",
        ),
        velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION => {
            (None, None, 1, "github_evidence_import_env_shape")
        }
        _ => return Ok(None),
    };
    if step_env.len() != expected_len
        || step_env
            .get(velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV)
            .is_none_or(String::is_empty)
        || token_key.is_some_and(|key| step_env.get(key).map(String::as_str) != token_value)
    {
        return Err(RenderError::InvalidWorkflow(error.to_owned()));
    }
    let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(operation.to_owned()))];
    env.extend(
        step_env
            .iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone()))),
    );
    Ok(Some(Yaml::Map(env)))
}
