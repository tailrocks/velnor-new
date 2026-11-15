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

impl Settings {
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
        }
    }
}
