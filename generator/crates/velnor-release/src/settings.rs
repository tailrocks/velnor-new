// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Settings {
    pub(crate) git_tag_name: Option<String>,
    pub(crate) git_release_draft: Option<bool>,
    pub(crate) release_always: Option<bool>,
    pub(crate) semver_check: Option<bool>,
    pub(crate) publish: Option<bool>,
    pub(crate) git_only: Option<bool>,
    pub(crate) publish_no_verify: Option<bool>,
    pub(crate) pr_branch_prefix: Option<String>,
    pub(crate) release: Option<bool>,
    pub(crate) git_tag_enable: Option<bool>,
    pub(crate) git_release_enable: Option<bool>,
    pub(crate) publish_timeout: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PackageEntry {
    pub(crate) name: String,
    #[serde(flatten)]
    pub(crate) settings: Settings,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Document {
    #[serde(default)]
    pub(crate) workspace: Settings,
    #[serde(default)]
    pub(crate) package: Vec<PackageEntry>,
}

const EXECUTABLE_KEYS: [&str; 2] = ["replace_command", "changelog_config"];

pub(crate) fn executable_key(value: &toml::Value) -> Option<&'static str> {
    match value {
        toml::Value::Table(table) => EXECUTABLE_KEYS
            .into_iter()
            .find(|key| table.contains_key(*key))
            .or_else(|| table.values().find_map(executable_key)),
        toml::Value::Array(items) => items.iter().find_map(executable_key),
        _ => None,
    }
}

impl Settings {
    pub(crate) fn publishes_to_crates_io(&self) -> bool {
        self.publish != Some(false) && self.git_only != Some(true)
    }

    pub(crate) fn publish_minutes(&self) -> u64 {
        self.publish_timeout
            .as_deref()
            .and_then(crate::timeout::minutes)
            .unwrap_or(crate::timeout::DEFAULT_PUBLISH_MINUTES)
    }

    pub(crate) fn registry_check_and_publish_minutes(&self) -> u64 {
        if self.publishes_to_crates_io() {
            self.publish_minutes().saturating_mul(2)
        } else {
            0
        }
    }

    pub(crate) fn over(&self, base: &Settings) -> Settings {
        Settings {
            git_tag_name: self
                .git_tag_name
                .clone()
                .or_else(|| base.git_tag_name.clone()),
            git_release_draft: self.git_release_draft.or(base.git_release_draft),
            release_always: self.release_always.or(base.release_always),
            semver_check: self.semver_check.or(base.semver_check),
            publish: self.publish.or(base.publish),
            git_only: self.git_only.or(base.git_only),
            publish_no_verify: self.publish_no_verify.or(base.publish_no_verify),
            pr_branch_prefix: self
                .pr_branch_prefix
                .clone()
                .or_else(|| base.pr_branch_prefix.clone()),
            release: self.release.or(base.release),
            git_tag_enable: self.git_tag_enable.or(base.git_tag_enable),
            git_release_enable: self.git_release_enable.or(base.git_release_enable),
            publish_timeout: self
                .publish_timeout
                .clone()
                .or_else(|| base.publish_timeout.clone()),
        }
    }
}
