// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PrivateSources, RepoPath, RepoView, RustRoot};

use crate::roots::CARGO_LOCK;
use crate::toml_file::table;

const GITHUB: &str = "github.com";

pub(crate) struct LockedGit {
    pub(crate) lock: RepoPath,
    pub(crate) package: String,
    pub(crate) owner: String,
    pub(crate) repository: String,
}

pub(crate) fn locked_packages(
    view: &RepoView,
    roots: &[RustRoot],
) -> Vec<(RepoPath, String, String)> {
    let mut found: Vec<(RepoPath, String, String)> = Vec::new();
    for root in roots {
        let lock = root.manifest.sibling(&CARGO_LOCK);
        let Some(document) = view.files.get(&lock).and_then(|bytes| table(bytes).ok()) else {
            continue;
        };
        let packages = document
            .get("package")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_table);
        for package in packages {
            let (Some(source), Some(name)) = (
                package.get("source").and_then(toml::Value::as_str),
                package.get("name").and_then(toml::Value::as_str),
            ) else {
                continue;
            };
            let entry = (lock.clone(), name.to_owned(), source.to_owned());
            if !found.contains(&entry) {
                found.push(entry);
            }
        }
    }
    found
}

pub(crate) fn github_repository(source: &str) -> Option<(&str, &str)> {
    let (_, tail) = source.strip_prefix("git+")?.split_once("://")?;
    let (authority, path) = tail.split_once('/')?;
    if !authority
        .rsplit('@')
        .next()
        .is_some_and(|host| host.eq_ignore_ascii_case(GITHUB))
    {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut parts = path.trim_end_matches('/').split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    let name = name.strip_suffix(".git").unwrap_or(name);
    (parts.next().is_none() && !owner.is_empty() && !name.is_empty()).then_some((owner, name))
}

pub(crate) fn mismatch(
    view: &RepoView,
    sources: &PrivateSources,
    roots: &[RustRoot],
) -> Option<String> {
    let packages = locked_packages(view, roots);
    let locked: Vec<LockedGit> = packages
        .iter()
        .filter_map(|(lock, package, source)| {
            let (owner, repository) = github_repository(source)?;
            Some(LockedGit {
                lock: lock.clone(),
                package: package.clone(),
                owner: owner.to_owned(),
                repository: repository.to_owned(),
            })
        })
        .collect();
    let listed = |repository: &str| {
        sources
            .repositories
            .iter()
            .any(|name| name.eq_ignore_ascii_case(repository))
    };
    let mut notes = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for git in locked
        .iter()
        .filter(|git| git.owner.eq_ignore_ascii_case(&sources.owner) && !listed(&git.repository))
    {
        let entry = format!(
            "{} ({}/{} in {})",
            git.package, git.owner, git.repository, git.lock
        );
        if !missing.contains(&entry) {
            missing.push(entry);
        }
    }
    if !missing.is_empty() {
        notes.push(format!(
            "[private_sources].repositories does not list {}, a git dependency of the token's owner; the token cannot read an unlisted private repository, so a cold fetch of it fails (list it, or ignore this note when the repository is public)",
            missing.join(", ")
        ));
    }
    if !packages.is_empty() {
        let unused: Vec<&String> = sources
            .repositories
            .iter()
            .filter(|name| {
                !locked.iter().any(|git| {
                    git.owner.eq_ignore_ascii_case(&sources.owner)
                        && git.repository.eq_ignore_ascii_case(name)
                })
            })
            .collect();
        if !unused.is_empty() {
            notes.push(format!(
                "[private_sources].repositories lists {}, which no Cargo.lock locks as a git dependency; the token reads more than the build fetches (remove it unless a build script or another source needs it)",
                unused
                    .iter()
                    .map(|name| format!("{}/{name}", sources.owner))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    (!notes.is_empty()).then(|| notes.join("; "))
}
