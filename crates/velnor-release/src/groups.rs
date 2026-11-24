// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fmt;

use crate::error::ReleaseError;
use crate::release_plz::{Package, Released};
use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupError {
    Leads {
        root: String,
        group: String,
        count: usize,
    },
    Tag {
        root: String,
        group: String,
        package: String,
    },
}

impl fmt::Display for GroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Leads { root, group, count } => write!(
                f,
                "{root}/release-plz.toml: version_group {group} has {count} members that tag and release, but members that turn their tag and release off need exactly one lead that owns both"
            ),
            Self::Tag {
                root,
                group,
                package,
            } => write!(
                f,
                "{root}/release-plz.toml: git_tag_name of {package} must match the lead of version_group {group}, so the group shares one tag"
            ),
        }
    }
}

impl From<GroupError> for ReleaseError {
    fn from(error: GroupError) -> Self {
        Self::Group(error)
    }
}

/// A follower of a `version_group` turns both its tag and its GitHub release
/// off; the lead of the group owns them.
pub(crate) fn is_follower(settings: &Settings) -> bool {
    settings.git_tag_enable == Some(false) && settings.git_release_enable == Some(false)
}

pub(crate) fn followed_groups(released: &[Released]) -> BTreeSet<String> {
    released
        .iter()
        .filter(|(_, settings, _)| is_follower(settings))
        .filter_map(|(_, _, group)| group.clone())
        .collect()
}

/// In a group with followers, exactly one member tags and releases, and every
/// member renders the lead's tag. Followers outside such a group stay refused.
pub(crate) fn check_groups(
    root: &str,
    released: &[Released],
    packages: &[Package],
) -> Result<(), ReleaseError> {
    let members = || released.iter().zip(packages);
    for group in packages
        .iter()
        .filter_map(|package| package.group.as_ref())
        .collect::<BTreeSet<_>>()
    {
        let leads: Vec<&Package> = members()
            .filter(|((_, settings, _), package)| {
                package.group.as_ref() == Some(group) && !is_follower(settings)
            })
            .map(|(_, package)| package)
            .collect();
        let [lead] = leads.as_slice() else {
            return Err(GroupError::Leads {
                root: root.to_owned(),
                group: group.clone(),
                count: leads.len(),
            }
            .into());
        };
        if let Some(other) = packages.iter().find(|package| {
            package.group.as_ref() == Some(group) && package.tag_pattern != lead.tag_pattern
        }) {
            return Err(GroupError::Tag {
                root: root.to_owned(),
                group: group.clone(),
                package: other.name.clone(),
            }
            .into());
        }
    }
    Ok(())
}
