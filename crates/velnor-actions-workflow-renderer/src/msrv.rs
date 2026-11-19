// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepRole};

use crate::RenderError;

pub(crate) fn check_no_msrv(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let leaking = id.to_lowercase().contains("msrv")
            || job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::MsrvQualification));
        if leaking {
            return Err(RenderError::InvalidWorkflow(format!(
                "msrv_in_pr_workflow:{id}"
            )));
        }
    }
    Ok(())
}
