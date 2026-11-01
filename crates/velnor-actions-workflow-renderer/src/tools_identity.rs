// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
use velnor_actions_contract::{Job, StepId, StepKind, StepRole};

use crate::RenderError;

const SHORT_DIGEST_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ToolsDigest(String);

impl ToolsDigest {
    fn parse(job: &str, input: &str) -> Result<Self, RenderError> {
        let short = input.get(..SHORT_DIGEST_LEN).unwrap_or_default();
        if short.len() != SHORT_DIGEST_LEN
            || !short
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "tools_cache_identity_digest_shape:{job}"
            )));
        }
        Ok(Self(input.to_owned()))
    }

    pub(crate) fn short(&self) -> &str {
        self.0.get(..SHORT_DIGEST_LEN).unwrap_or_default()
    }
}

pub(crate) fn tools_digest(id: &str, job: &Job) -> Result<Option<ToolsDigest>, RenderError> {
    let restores = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheRestore))
        .collect::<Vec<_>>();
    let identities = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ToolsCacheIdentity))
        .collect::<Vec<_>>();
    if restores.is_empty() && identities.is_empty() {
        return Ok(None);
    }
    if restores.len() != 1 || identities.len() != 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_restore_shape:{id}"
        )));
    }
    crate::cache_steps::validate_tools_restore_call(restores[0])
        .map_err(|_| RenderError::InvalidWorkflow(format!("tools_cache_restore_shape:{id}")))?;
    let identity = identities[0];
    let StepKind::Action {
        uses,
        with: identity_with,
        env: identity_env,
    } = &identity.kind
    else {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_identity_shape:{id}"
        )));
    };
    if identity.id != Some(StepId::ToolsCacheIdentity) || identity.condition.is_some() {
        return Err(RenderError::InvalidWorkflow(format!(
            "tools_cache_identity_shape:{id}"
        )));
    }
    crate::cache_p08::validate_runtime_identity_action(
        identity,
        uses,
        &job.runs_on,
        identity_with,
        identity_env,
    )
    .map_err(|_| RenderError::InvalidWorkflow(format!("tools_cache_identity_shape:{id}")))?;
    let input = identity_with
        .get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("tools_cache_identity_digest_missing:{id}"))
        })?;
    ToolsDigest::parse(id, input).map(Some)
}
