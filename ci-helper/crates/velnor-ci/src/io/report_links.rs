// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::inputs::{SYMLINK_MODE, SavedInput, Tracked, TrackedFile};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

#[derive(Default)]
pub(super) struct LinkEvidence {
    pub changed: BTreeSet<String>,
    pub unverifiable: bool,
}

impl LinkEvidence {
    pub(super) fn collect(current: &Tracked, saved: Option<&[SavedInput]>) -> Self {
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
                referent(&link.path, &current_files),
                referent(&link.path, &saved_files),
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
    path: &str,
    files: &BTreeMap<&str, &'a TrackedFile>,
) -> Option<Vec<&'a TrackedFile>> {
    let mut path = path.to_owned();
    let mut visited = BTreeSet::new();
    let mut chain = Vec::new();
    loop {
        if !visited.insert(path.clone())
            || Path::new(&path).ancestors().skip(1).any(|ancestor| {
                ancestor
                    .to_str()
                    .and_then(|path| files.get(path))
                    .is_some_and(|file| file.mode == SYMLINK_MODE)
            })
        {
            return None;
        }
        let file = *files.get(path.as_str())?;
        chain.push(file);
        if file.mode != SYMLINK_MODE {
            return Some(chain);
        }
        path = target(&path, &file.blob, files)?;
    }
}

fn target(path: &str, text: &str, files: &BTreeMap<&str, &TrackedFile>) -> Option<String> {
    let parent = Path::new(path).parent()?;
    let mut parts = Vec::new();
    let joined = parent.join(text);
    let mut components = joined.components().peekable();
    while let Some(part) = components.next() {
        match part {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
        if components.peek().is_some() && files.contains_key(parts.join("/").as_str()) {
            return None;
        }
    }
    Some(parts.join("/"))
}
