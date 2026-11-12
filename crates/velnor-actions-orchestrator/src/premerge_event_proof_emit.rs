// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, prepare::GenerationPreparation};
use velnor_actions_workflow_renderer::render::RenderedFile;

pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(config) = &prep.config.premerge_event_proof else {
        return Ok(Vec::new());
    };
    let repository = crate::release_identity::origin_repository(&prep.root)?;
    Ok(vec![
        velnor_actions_workflow_renderer::premerge_event_proof::render(
            config,
            &repository,
            &prep.default_branch,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}
