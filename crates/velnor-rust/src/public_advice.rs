// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{GitRepository, RepoView, RustCiConfig, RustRoot};

use crate::private_lock::locked_packages;
use crate::private_sources::declared_public;
use crate::public_git::pinned_github;

/// Notes about commit-pinned github.com git dependencies and `[public_sources]`.
pub(crate) fn notes(view: &RepoView, config: &RustCiConfig, roots: &[RustRoot]) -> Vec<String> {
    let declared = declared_public(config);
    let packages = locked_packages(view, roots);
    let pinned: Vec<(String, String, GitRepository)> = packages
        .iter()
        .filter_map(|(lock, package, source)| {
            let git = pinned_github(source)?;
            Some((
                lock.to_string(),
                package.clone(),
                GitRepository {
                    owner: git.owner.to_owned(),
                    name: git.name.to_owned(),
                },
            ))
        })
        .collect();
    let private_owner = config
        .private_sources
        .as_ref()
        .map(|sources| sources.owner.as_str());
    let mut notes = Vec::new();
    if let Some((lock, package, git)) = pinned.iter().find(|(_, _, git)| {
        !declared
            .iter()
            .any(|known| known.matches(&git.owner, &git.name))
            && private_owner.is_none_or(|owner| !git.owner.eq_ignore_ascii_case(owner))
    }) {
        let (owner, name) = (&git.owner, &git.name);
        notes.push(format!(
            "{lock} locks {package} from github.com/{owner}/{name} at a full commit; if that repository is public, add [public_sources] git = ['https://github.com/{owner}/{name}'] to .velnor/rust-ci.toml so the base caches, the pull-request deltas and cache.mbx_remote work (velnor-ci checks on every run, without a token, that it is public); if it is private, add a [private_sources] table instead (see CONFIG.md)"
        ));
    }
    if !packages.is_empty() {
        let unused: Vec<String> = declared
            .iter()
            .filter(|known| {
                !pinned
                    .iter()
                    .any(|(_, _, git)| known.matches(&git.owner, &git.name))
            })
            .map(|known| format!("{}/{}", known.owner, known.name))
            .collect();
        if !unused.is_empty() {
            notes.push(format!(
                "[public_sources].git lists {}, which no Cargo.lock locks as a git dependency pinned to a full commit on github.com; remove it",
                unused.join(", ")
            ));
        }
    }
    notes
}
