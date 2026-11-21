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

pub(in crate::io) fn build_unit_files(
    workspaces: &[Workspace],
    directory: &Path,
) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    let touching: Vec<&Workspace> = workspaces
        .iter()
        .filter(|workspace| {
            same(&workspace.target_directory, directory)
                || same(&workspace.build_directory, directory)
        })
        .collect();
    let plain = !touching.is_empty()
        && touching.iter().all(|workspace| {
            same(&workspace.target_directory, directory)
                && same(&workspace.build_directory, directory)
        });
    if plain {
        return unit_files(directory);
    }
    unchecked_unit_files(directory)
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
    let target_scan = scan(target, limit, inside(target, build))?;
    let build_scan = scan(build, limit, inside(build, target))?;
    let (in_target, target_built) = (target_scan.found, target_scan.built);
    let (mut found, build_built) = (build_scan.found, build_scan.built);
    found.extend(
        in_target
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

fn same(left: &Path, right: &Path) -> bool {
    left == right || physical(left.to_path_buf()) == physical(right.to_path_buf())
}

impl From<Printed> for Workspace {
    fn from(printed: Printed) -> Self {
        let target_directory = physical(printed.target_directory);
        Self {
            build_directory: printed
                .build_directory
                .map_or_else(|| target_directory.clone(), physical),
            root: printed.workspace_root,
            target_directory,
        }
    }
}

impl Workspace {
    pub(in crate::io) fn directories(&self) -> Vec<PathBuf> {
        let mut directories = vec![self.target_directory.clone()];
        if !same(&self.build_directory, &self.target_directory) {
            directories.push(self.build_directory.clone());
        }
        directories
    }
}

impl Workspace {
    pub(in crate::io) fn units(&self) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        unit_files_apart(&self.target_directory, &self.build_directory)
    }

    pub(in crate::io) fn build_units(
        &self,
        workspaces: &[Workspace],
    ) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        if same(&self.target_directory, &self.build_directory) {
            return build_unit_files(workspaces, &self.build_directory);
        }
        Ok(apart(&self.target_directory, &self.build_directory)?.found)
    }
}
