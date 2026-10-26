// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::RenderedTree;
pub use velnor_actions_workflow_renderer::owned_tool_publication::SourceQualificationTrigger;

use crate::{
    OrchestratorError, config::load_config, generate, owned_tool_publication,
    prepare::check_velnor_identity, resolve_root,
};

pub fn preview_owned_tool_candidates(
    root: &Path,
    destination: &Path,
    trigger: SourceQualificationTrigger,
) -> Result<Vec<String>, OrchestratorError> {
    let root = resolve_root(root)?;
    let config = load_config(&root)?;
    if config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Err(OrchestratorError::IdentityRejected {
            problem: "owned_tool_candidates_require_velnor_repository_policy".to_owned(),
        });
    }
    check_velnor_identity(&root, &config)?;
    let files = owned_tool_publication::files_for_trigger(&root, trigger)?;
    if files.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "owned_tool_source_approvals_absent".to_owned(),
        });
    }
    let tree = RenderedTree {
        files,
        symlinks: Vec::new(),
    };
    let preview = generate::guards::prepare_preview_dir(&root, destination)?;
    generate::write_tree(&preview.join(".github"), &tree)?;
    Ok(tree.paths())
}
