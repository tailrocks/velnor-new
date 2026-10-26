// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_actionlint::render_jactionlint_yaml;
use velnor_actions_contract::{ExecutionMode, expand_workflow};
use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};
use velnor_actions_workflow_renderer::render::RenderedTree;
use velnor_actions_workflow_renderer::steps::rehead_lint_config_marker;
use velnor_actions_workflow_renderer::tree::render_tree_with_claude_mode;

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::workflow::scale_set_selector_for_dispatch;

use super::{cache_cleanup_emit, mise_task_suites_emit, runner_labels, rust_install_audit};

pub(super) fn render_all(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    if crate::homebrew::source_policy::requires_native_generation_policy(
        prep.config.distribution.homebrew_tap.as_ref(),
    ) {
        crate::homebrew::source_policy::check_preparation(prep)?;
    }
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let ir = expand_workflow(&prep.workflow.ir, &prep.config, dispatch).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let mut render_context = prep.workflow.context.clone();
    render_context.scale_set_selector = scale_set_selector_for_dispatch(&prep.config, dispatch)?;
    let mut lint_input = prep.workflow.lint_config.clone();
    lint_input.runner_labels = runner_labels::collect(
        &prep.config,
        &ir,
        render_context.scale_set_selector.as_ref(),
    );
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &render_context,
        &mise,
    )?;
    let workflow = rendered.yaml;
    let verifier_actions = crate::workflow::verifier_actions::VerifierActionSet::build(
        &prep.emission_catalog(),
        version,
    )?;
    let lint_config = rehead_lint_config_marker(&render_jactionlint_yaml(&lint_input)?, version)?;
    let mut extra = crate::release_emit::release_files(prep, &mise)?;
    extra.extend(crate::publish_emit::publish_files(prep, &mise)?);
    extra.extend(crate::binary_release_emit::files(prep)?);
    extra.extend(crate::binary_preview_emit::files(prep)?);
    extra.extend(crate::freshness_emit::freshness_files(prep)?);
    extra.extend(crate::freshness_emit::upgrades::upgrades_files(prep)?);
    extra.extend(cache_cleanup_emit::files(prep)?);
    extra.extend(crate::owned_tool_publication::files(prep)?);
    extra.extend(crate::tofu_apply_emit::tofu_apply_files(prep)?);
    extra.extend(crate::review_completion_workflow_emit::files(prep)?);
    extra.extend(crate::routing::extra_files(
        &prep.config,
        &prep.emission_catalog(),
        version,
    )?);
    extra.extend(crate::rust_suites_emit::suite_workflow_files(prep)?);
    extra.extend(mise_task_suites_emit::suite_workflow_files(prep)?);
    extra.extend(verifier_actions.files());
    if let Some(dco) = &prep.config.dco {
        extra.push(velnor_actions_workflow_renderer::dco::render_dco_yml(
            dco, version,
        )?);
    }
    extra.extend(rendered.shared);
    let mut audit_files = extra.clone();
    audit_files.push(velnor_actions_workflow_renderer::render::RenderedFile {
        path: velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned(),
        bytes: workflow.clone(),
    });
    rust_install_audit::audit_with_verifiers(&audit_files, &verifier_actions)?;
    let tree = render_tree_with_claude_mode(
        &workflow,
        &lint_config,
        &extra,
        version,
        prep.config.agents.claude_md,
    )?;
    Ok(tree)
}

pub(super) fn check_tree_paths(
    tree: &RenderedTree,
) -> Result<Vec<SafeTreePath>, OrchestratorError> {
    let mut safe = Vec::with_capacity(tree.files.len() + tree.symlinks.len());
    for file in &tree.files {
        safe.push(guard::validate_tree_path(&file.path)?);
    }
    for link in &tree.symlinks {
        safe.push(guard::validate_tree_path(&link.path)?);
    }
    Ok(safe)
}
