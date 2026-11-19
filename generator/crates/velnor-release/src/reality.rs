// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::plan::{ReleaseFiles, manifest_of};
use crate::velnor_config::VelnorConfig;

pub(crate) fn check_reality(
    config: &VelnorConfig,
    files: &ReleaseFiles,
) -> Result<(), ReleaseError> {
    if let Some(manifests) = &files.cargo_manifests
        && let Some(root) = config
            .roots
            .iter()
            .find(|root| !manifests.contains(&manifest_of(root)))
    {
        return Err(ReleaseError::RootWithoutCargoManifest { root: root.clone() });
    }
    if let Some(names) = &files.workflow_names
        && let Some(unknown) = config.after.iter().find(|name| !names.contains(*name))
    {
        return Err(ReleaseError::UnknownWorkflow {
            name: unknown.clone(),
        });
    }
    if let Some(pushed) = &files.push_workflows
        && let Some(missing) = pushed.iter().find(|name| !config.after.contains(*name))
    {
        return Err(ReleaseError::AfterMissingWorkflow {
            name: missing.clone(),
        });
    }
    Ok(())
}
