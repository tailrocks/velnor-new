//! Pure effective registry-publication policy construction.
use velnor_actions_contract::config::RustReleaseConfig;
use velnor_actions_rust::release_select::ReleaseSelection;
use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};

/// Effective normal-policy config: tag plus allowlist, no features.
///
/// The schema carries no `publish_features`, so emission sets none;
/// `semver_check` stays true per the release contract.
pub(super) fn plz_config(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> ReleasePlzConfig {
    ReleasePlzConfig {
        tag_pattern: release.tag_name.clone(),
        semver_check: true,
        packages: selection
            .packages
            .iter()
            .map(|package| ReleasePlzPackage {
                name: package.name.clone(),
                publish_features: Vec::new(),
            })
            .collect(),
    }
}
