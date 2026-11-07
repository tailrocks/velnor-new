// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Path, TestResult, release};

pub(super) fn check_historical_workspace_lock(
    root: &Path,
    prep: &release::GenerationPreparation,
    release_config: &release::RustReleaseConfig,
    derived: &release::ReleaseIntentInputs,
) -> TestResult<velnor_actions_workflow_renderer::release_spec::lock::ReleaseConcurrency> {
    let snapshot = derived
        .source_snapshot
        .as_ref()
        .ok_or("source snapshot missing")?;
    let source_lock = crate::release::release_identity::release_lock(
        &derived.metadata,
        snapshot.root(),
        &release_config.manifest_path,
        &derived.known,
        &derived.registry,
        &derived.repository,
    )?;
    let live_metadata = release::release_metadata_json(prep, &release_config.manifest_path)?;
    let live_known = release::selection::tracked_manifests(root)?;
    let live_lock = crate::release::release_identity::release_lock(
        &live_metadata,
        root,
        &release_config.manifest_path,
        &live_known,
        &derived.registry,
        &derived.repository,
    )?;
    assert_eq!(source_lock, live_lock);
    Ok(source_lock)
}
