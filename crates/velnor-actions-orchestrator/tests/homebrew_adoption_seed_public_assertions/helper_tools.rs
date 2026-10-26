// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error, io};

use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_orchestrator::GenerationPreparation;
use velnor_actions_workflow_renderer::{FINAL_JOB_ID, PLAN_JOB_ID};

pub(super) fn assert_published_helper_tools(
    preparation: &GenerationPreparation,
    jobs: &BTreeMap<String, Job>,
) -> Result<(), Box<dyn Error>> {
    for key in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
        assert_eq!(
            preparation
                .workflow
                .context
                .plan_consumer_env
                .get(key)
                .map(String::as_str),
            Some("false")
        );
    }
    for id in [PLAN_JOB_ID, FINAL_JOB_ID] {
        let job = jobs
            .get(id)
            .ok_or_else(|| io::Error::other(format!("missing helper job {id}")))?;
        let prepare = job
            .steps
            .iter()
            .filter(|step| step.role == Some(StepRole::PreparePinnedTools))
            .collect::<Vec<_>>();
        assert_eq!(prepare.len(), 1, "{id} must have one tool preparation");
        let StepKind::Shell { run, env, .. } = &prepare[0].kind else {
            return Err(io::Error::other(format!("{id} preparation is not native shell")).into());
        };
        assert_eq!(
            run.iter()
                .filter(|argument| *argument == "gh@2.103.2")
                .count(),
            1,
            "{id} omitted the published helper55 GH dependency"
        );
        assert!(
            !run.contains(&format!("gh@{}", velnor_actions_mise::catalog::GH_VERSION)),
            "{id} substituted candidate GH for helper55 GH"
        );
        assert!(
            !run.iter()
                .any(|argument| argument.contains("VELNOR_TOOL_GH")),
            "{id} selected project-controlled helper GH"
        );
        for key in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
            assert!(
                env.get(key).is_none_or(|value| value == "false"),
                "{id} overrides {key}"
            );
        }
    }
    Ok(())
}
