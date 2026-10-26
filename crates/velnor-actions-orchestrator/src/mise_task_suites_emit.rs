// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::{
    MiseTaskSuitePolicy, MiseTaskSuitesRenderSpec, RenderedFile, render_mise_task_suites,
};

use crate::OrchestratorError;
use crate::pins::resolve_verification_mise_setup;
use crate::prepare::GenerationPreparation;

pub(crate) fn suite_workflow_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let mut suites = Vec::with_capacity(prep.config.workflow.task_suites.len());
    for suite in &prep.config.workflow.task_suites {
        let resolved = crate::verification_tasks::resolve_mise_task_tools(
            &prep.discovery.tool_checks,
            &suite.source,
            &suite.mise_task,
            suite.runner,
        )?;
        let evidence = if let Some(input) = &suite.evidence_snapshot {
            let fetch_spec = input.resolve_spec(
                &prep.config.workflow.task_suites,
                &suite.id,
                &prep.default_branch,
            )?;
            let helper = crate::pins::consumer_evidence_helper(
                suite.runner.runs_on(),
                env!("CARGO_PKG_VERSION"),
                &prep.discovery,
            )?;
            Some((fetch_spec, helper))
        } else {
            None
        };
        suites.push(MiseTaskSuitePolicy {
            suite: suite.clone(),
            runner_label: suite.runner.runs_on().to_owned(),
            mise_setup: resolve_verification_mise_setup(&prep.config, suite.runner)?,
            selected_tools: resolved.selected_tools,
            mise_config_sha256: resolved.mise_config_sha256,
            mise_lock_sha256: resolved.mise_lock_sha256,
            rust_toolchain_sha256: resolved.rust_toolchain_sha256,
            evidence_fetch_spec: evidence.as_ref().map(|(spec, _)| spec.clone()),
            evidence_helper: evidence.map(|(_, helper)| helper),
        });
    }
    let spec = MiseTaskSuitesRenderSpec {
        suites,
        default_branch: prep.default_branch.clone(),
        checkout_uses: crate::workflow::CHECKOUT_USES.to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    render_mise_task_suites(&spec).map_err(Into::into)
}
