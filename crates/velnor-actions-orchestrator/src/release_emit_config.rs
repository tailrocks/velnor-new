// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::RustReleaseConfig;
use velnor_actions_rust::release_select::ReleaseSelection;
use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};

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
