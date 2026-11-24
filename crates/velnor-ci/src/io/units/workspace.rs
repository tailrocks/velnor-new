// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{UnitFile, Workspace, built, directory_limit, scan, unit_files};
use crate::error::CiError;

#[derive(Deserialize)]
pub(super) struct Printed {
    workspace_root: PathBuf,
    target_directory: PathBuf,
    build_directory: Option<PathBuf>,
}

pub(in crate::io) fn unchecked_unit_files(
    target: &Path,
) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    Ok(scan(target, directory_limit()?, None)?.found)
}

pub(in crate::io) struct Roots {
    resolved: Vec<[PathBuf; 2]>,
}

impl Roots {
    pub(in crate::io) fn new(workspaces: &[Workspace]) -> Self {
        Self {
            resolved: workspaces
                .iter()
                .map(|workspace| {
                    [&workspace.target_directory, &workspace.build_directory]
                        .map(|directory| physical(directory.clone()))
                })
                .collect(),
        }
    }

    pub(in crate::io) fn scan(
        &self,
        directory: &Path,
        checked: bool,
    ) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        if checked {
            self.unit_files(directory)
        } else {
            unchecked_unit_files(directory)
        }
    }

    pub(in crate::io) fn unit_files(
        &self,
        directory: &Path,
    ) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        let wanted = physical(directory.to_path_buf());
        let touching: Vec<&[PathBuf; 2]> = self
            .resolved
            .iter()
            .filter(|resolved| resolved.contains(&wanted))
            .collect();
        let plain = !touching.is_empty()
            && touching
                .iter()
                .all(|resolved| resolved.iter().all(|path| *path == wanted));
        if plain {
            return unit_files(directory);
        }
        unchecked_unit_files(directory)
    }
}

fn inside<'a>(outer: &Path, inner: &'a Path) -> Option<&'a Path> {
    inner.starts_with(outer).then_some(inner)
}

struct Apart {
    found: Vec<(UnitFile, PathBuf)>,
    built: bool,
}

fn apart(target: &Path, build: &Path) -> Result<Apart, CiError> {
    let limit = directory_limit()?;
    let in_target = scan(target, limit, inside(target, build))?;
    let in_build = scan(build, limit, inside(build, target))?;
    let (target_built, build_built) = (in_target.built, in_build.built);
    let mut found = in_build.found;
    found.extend(
        in_target
            .found
            .into_iter()
            .filter(|(unit, _)| matches!(unit, UnitFile::Directory { .. })),
    );
    Ok(Apart {
        found,
        built: target_built || build_built,
    })
}

pub(in crate::io) fn unit_files_apart(
    target: &Path,
    build: &Path,
) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    if same(target, build) {
        return unit_files(target);
    }
    let Apart { found, built } = apart(target, build)?;
    built::refuse_without_units(build, built, &found)?;
    Ok(found)
}

fn physical(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

pub(in crate::io) fn same(left: &Path, right: &Path) -> bool {
    left == right || physical(left.to_path_buf()) == physical(right.to_path_buf())
}

impl From<Printed> for Workspace {
    fn from(printed: Printed) -> Self {
        Self {
            build_directory: printed
                .build_directory
                .unwrap_or_else(|| printed.target_directory.clone()),
            root: printed.workspace_root,
            target_directory: printed.target_directory,
        }
    }
}

impl Workspace {
    pub(in crate::io) fn shares_build_directory(&self) -> bool {
        same(&self.build_directory, &self.target_directory)
    }

    pub(in crate::io) fn same_directories(&self, other: &Self) -> bool {
        self.root == other.root
            && same(&self.target_directory, &other.target_directory)
            && same(&self.build_directory, &other.build_directory)
    }

    pub(in crate::io) fn directories(&self) -> Vec<PathBuf> {
        let mut directories = vec![self.target_directory.clone()];
        if !self.shares_build_directory() {
            directories.push(self.build_directory.clone());
        }
        directories
    }
}

impl Workspace {
    pub(in crate::io) fn build_units(
        &self,
        workspaces: &[Workspace],
    ) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        if self.shares_build_directory() {
            return Roots::new(workspaces).unit_files(&self.build_directory);
        }
        Ok(apart(&self.target_directory, &self.build_directory)?.found)
    }
}
