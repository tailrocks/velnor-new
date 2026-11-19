// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Entries, Kind, resolve};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::io::audit) enum State {
    File(PathBuf),
    Directory(PathBuf),
    Missing,
}

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::io::audit) struct Evidence {
    pub(in crate::io::audit) entries: Entries,
    pub(in crate::io::audit) declared: BTreeMap<PathBuf, State>,
    pub(in crate::io::audit) targets: BTreeSet<PathBuf>,
}

impl Evidence {
    pub(in crate::io::audit) fn state(&self, path: &Path) -> Option<State> {
        self.declared
            .get(path)
            .cloned()
            .or_else(|| self.inferred(path))
    }

    fn inferred(&self, path: &Path) -> Option<State> {
        if let Some(root) = resolve(&self.entries, path) {
            return Some(State::Directory(root));
        }
        let parent = path.parent()?;
        let Some(root) = resolve(&self.entries, parent) else {
            return matches!(self.inferred(parent), Some(State::Missing)).then_some(State::Missing);
        };
        let name = Path::new(path.file_name()?);
        match self.entries.get(&root)?.get(name) {
            Some(Kind::File | Kind::Other) => Some(State::File(root.join(name))),
            Some(Kind::Link {
                resolved: Some(resolved),
                ..
            }) => Some(State::File(resolved.clone())),
            Some(Kind::Directory | Kind::Link { resolved: None, .. }) => None,
            None => Some(State::Missing),
        }
    }
}

pub(in crate::io::audit) fn state(path: &Path) -> io::Result<State> {
    match fs::metadata(path) {
        Ok(metadata) => {
            let path = fs::canonicalize(path)?;
            Ok(if metadata.is_dir() {
                State::Directory(path)
            } else {
                State::File(path)
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(State::Missing),
        Err(error) => Err(error),
    }
}
