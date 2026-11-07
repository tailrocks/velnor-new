// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const RELEASE_HOSTED_RUNNER: &str = "ubuntu-26.04";
use velnor_actions_contract::CRATE_RELEASE_WORKFLOW_PATH;
pub const RELEASE_INTENT_PATH: &str = ".github/velnor-release-intent.toml";
pub const RELEASE_CONFIG_PATH: &str = ".github/release-plz.toml";
pub const RELEASE_SOURCE_DIR: &str = "release-source";
pub const RELEASE_CURRENT_PATHS: &[&str] = &[
    RELEASE_INTENT_PATH,
    RELEASE_CONFIG_PATH,
    CRATE_RELEASE_WORKFLOW_PATH,
];
pub const RELEASE_TREE_PATHS: &[&str] = &[
    ".github/release-plz-bootstrap.toml",
    RELEASE_INTENT_PATH,
    ".github/release-plz-publish.toml",
    RELEASE_CONFIG_PATH,
    CRATE_RELEASE_WORKFLOW_PATH,
];

#[must_use]
pub fn release_stale_paths() -> &'static [&'static str] {
    RELEASE_TREE_PATHS
}
