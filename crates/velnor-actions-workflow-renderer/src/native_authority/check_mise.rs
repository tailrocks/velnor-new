// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NativeAuthorityError, NativeAuthorityInput, invalid};
use std::collections::BTreeMap;
use velnor_actions_contract::{ReleaseTarget, WorkflowPolicy};

#[derive(Debug, Clone, Default)]
pub(super) struct CheckMiseSetups {
    checks: BTreeMap<String, (ReleaseTarget, crate::MiseSetup)>,
}

impl CheckMiseSetups {
    pub(super) fn build(input: &NativeAuthorityInput<'_>) -> Result<Self, NativeAuthorityError> {
        if input.config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
            return Ok(Self::default());
        }
        let lock = input
            .published_generator_lock
            .ok_or_else(|| invalid("required_verdict_requires_published_generator_lock"))?;
        let mut checks = BTreeMap::new();
        for check in &input.config.checks {
            let target = check.runner.platform.release_target();
            let current = super::tools::mise_setup(input.config, target)?;
            let setup = match input.approved_release {
                Some(approved) => current.for_approved_generator(lock, target, &approved)?,
                None => return Err(invalid("controller_approvals:missing_approval")),
            };
            checks.insert(format!("check-{}", check.id), (target, setup));
        }
        Ok(Self { checks })
    }

    pub(super) fn for_job(
        &self,
        id: &str,
        target: &str,
    ) -> Result<Option<crate::MiseSetup>, crate::RenderError> {
        let entry = self.checks.get(id).or_else(|| {
            id.strip_suffix(velnor_actions_contract::HOSTED_SUFFIX)
                .or_else(|| id.strip_suffix(velnor_actions_contract::SCALE_SUFFIX))
                .and_then(|base| self.checks.get(base))
        });
        let Some((platform, setup)) = entry else {
            return Ok(None);
        };
        if platform.triple() != target {
            return Err(crate::RenderError::InvalidWorkflow(
                "native_authority_check_mise_target".to_owned(),
            ));
        }
        Ok(Some(setup.clone()))
    }
}
