// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{HelperRelease, RepoPath, RepoView, WORKFLOWS_CONFIG_PATH};
use velnor_render::render_reusable;
use velnor_workflows::{WorkflowsError, WorkflowsPlan};

use crate::GenerateError;

pub(crate) fn reusable_workflows(
    view: &RepoView,
    helper: &HelperRelease,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    let Some(bytes) = view.files.get(&WORKFLOWS_CONFIG_PATH) else {
        return Ok((BTreeMap::new(), Vec::new()));
    };
    let text = std::str::from_utf8(bytes)
        .map_err(|_| GenerateError::ReleaseFileNotUtf8(WORKFLOWS_CONFIG_PATH))?;
    let plan = WorkflowsPlan::read(text).map_err(GenerateError::Workflows)?;
    let files = render_reusable(&plan, helper).map_err(GenerateError::Rules)?;
    let files = files
        .into_iter()
        .map(|(path, text)| {
            let path = RepoPath::new(path.as_str()).map_err(|error| {
                GenerateError::Workflows(WorkflowsError::invalid(error.to_string()))
            })?;
            Ok((path, text))
        })
        .collect::<Result<BTreeMap<_, _>, GenerateError>>()?;
    Ok((files, plan.warnings()))
}
