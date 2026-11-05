// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    IntentSettings, check_release_selection, check_recovery_record, common_registry,
    crate_release_intent, plz_config, reviewed_generator_manifest, reviewed_pr_write_paths,
    selection,
};
use crate::OrchestratorError;
use velnor_actions_contract::{
    WorkflowPolicy,
    config::{CrateReleaseIntent, ReleaseRegistry},
};

pub(crate) fn derive_intent_from_snapshot(
    source: &crate::crate_release_qualification_snapshot::SourceSnapshot,
    config: &velnor_actions_contract::VelnorConfig,
    metadata: &str,
    manifest: &[u8],
    metadata_config: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
    repository: &str,
    default_branch: &str,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    let invalid =
        || OrchestratorError::unsupported("crate_release_qualification", "invalid reviewed inputs");
    let release = config
        .stacks
        .rust
        .as_ref()
        .map(|stack| &stack.release)
        .ok_or_else(invalid)?;
    if config.workflow.policy != WorkflowPolicy::ConsumerV1
        || !release.enabled
        || release.registry != ReleaseRegistry::CratesIo
        || metadata_config.manifest_path != release.manifest_path
        || release
            .recovery
            .as_ref()
            .is_some_and(|record| record.source_sha != source.source_sha())
        || release
            .new_crates
            .as_ref()
            .is_some_and(|record| record.source_sha != source.source_sha())
    {
        return Err(invalid());
    }
    let catalog = crate::tool_catalog_resolve::resolve_snapshot_catalog(
        source.root(),
        config,
        source.nested_toolchains(),
    )?;
    if metadata_config.rust_version != catalog.version(velnor_actions_mise::PinnedTool::Rust) {
        return Err(invalid());
    }
    let selection =
        selection::select_with_root(source.root(), release, metadata, source.manifests())?;
    if selection.is_empty() {
        return Err(invalid());
    }
    check_recovery_record(release, &selection)?;
    check_release_selection(release, &selection)?;
    let registry = common_registry(&selection)?;
    let pin = reviewed_generator_manifest(
        Some(std::str::from_utf8(manifest).map_err(|_| invalid())?),
        false,
        env!("CARGO_PKG_VERSION"),
    )?;
    let config = plz_config(release, &selection)?;
    let paths = reviewed_pr_write_paths(source.root(), release, metadata, source.manifests())?;
    let intent = crate_release_intent(
        &IntentSettings {
            default_branch,
            catalog: &catalog,
        },
        release,
        repository,
        &registry,
        (&pin.0, &pin.1),
        &config,
        paths,
    );
    intent.validate().map_err(|_| invalid())?;
    Ok(intent)
}
