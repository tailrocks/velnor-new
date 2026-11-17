// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::inputs::{SYMLINK_MODE, SavedInput, Tracked, TrackedFile};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

#[derive(Default)]
pub(super) struct LinkEvidence {
    pub changed: BTreeSet<String>,
    pub unverifiable: bool,
}

impl LinkEvidence {
    pub(super) fn collect(
        checkout: &Path,
        current: &Tracked,
        saved: Option<&[SavedInput]>,
    ) -> Self {
        let current_files: BTreeMap<_, _> = current
            .files
            .iter()
            .chain(&current.links)
            .map(|file| (file.path.as_str(), file))
            .collect();
        let saved_files: BTreeMap<_, _> = saved
            .into_iter()
            .flatten()
            .map(|input| (input.file.path.as_str(), &input.file))
            .collect();
        let mut evidence = Self {
            unverifiable: current.unverifiable_links > 0,
            ..Self::default()
        };
        for link in &current.links {
            match (
                referent(checkout, &link.path, &current_files),
                referent(checkout, &link.path, &saved_files),
            ) {
                (Some(current), Some(saved)) if current != saved => {
                    evidence.changed.insert(link.path.clone());
                }
                (Some(_), Some(_)) => {}
                _ => evidence.unverifiable = true,
            }
        }
        evidence
    }
}

fn referent<'a>(
    checkout: &Path,
    path: &str,
    files: &BTreeMap<&str, &'a TrackedFile>,
) -> Option<Vec<&'a TrackedFile>> {
    let mut path = resolve(checkout, Path::new(path), files)?;
    let mut visited = BTreeSet::new();
    let mut chain = Vec::new();
    loop {
        if !visited.insert(path.clone()) {
            return None;
        }
        let file = *files.get(path.as_str())?;
        chain.push(file);
        if file.mode != SYMLINK_MODE {
            return Some(chain);
        }
        if file.blob.ends_with('/') || file.blob.split('/').next_back() == Some(".") {
            return None;
        }
        path = resolve(
            checkout,
            &Path::new(&path).parent()?.join(&file.blob),
            files,
        )?;
    }
}

fn resolve(checkout: &Path, path: &Path, files: &BTreeMap<&str, &TrackedFile>) -> Option<String> {
    let mut parts = Vec::new();
    let mut components = path.components().peekable();
    while let Some(part) = components.next() {
        match part {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
        if components.peek().is_some() {
            let prefix = parts.join("/");
            if files.contains_key(prefix.as_str())
                || !fs::symlink_metadata(checkout.join(prefix)).ok()?.is_dir()
            {
                return None;
            }
        }
    }
    Some(parts.join("/"))
}
