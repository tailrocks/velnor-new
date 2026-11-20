// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
    Ok(scan(target, directory_limit()?)?.0)
}

pub(in crate::io) fn unit_files_apart(
    target: &Path,
    build: &Path,
) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    if target == build {
        return unit_files(target);
    }
    let limit = directory_limit()?;
    let (mut found, target_built) = scan(target, limit)?;
    let (more, build_built) = scan(build, limit)?;
    found.extend(more);
    built::refuse_without_units(build, target_built || build_built, &found)?;
    Ok(found)
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
    pub(in crate::io) fn directories(&self) -> Vec<PathBuf> {
        let mut directories = vec![self.target_directory.clone()];
        if self.build_directory != self.target_directory {
            directories.push(self.build_directory.clone());
        }
        directories
    }
}

impl Workspace {
    pub(in crate::io) fn units(&self) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        unit_files_apart(&self.target_directory, &self.build_directory)
    }

    pub(in crate::io) fn unchecked_units(&self) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
        let mut found = Vec::new();
        for directory in self.directories() {
            found.extend(unchecked_unit_files(&directory)?);
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::Workspace;
    use std::path::Path;

    fn parsed(extra: &str) -> Workspace {
        serde_json::from_str(&format!(
            "{{\"workspace_root\":\"/w\",\"target_directory\":\"/w/target\"{extra},\"packages\":[]}}"
        ))
        .unwrap_or_else(|e| unreachable!("{e}"))
    }

    #[test]
    fn the_build_directory_is_the_one_cargo_metadata_prints() {
        let workspace = parsed(",\"build_directory\":\"/w/build\"");
        assert_eq!(workspace.build_directory, Path::new("/w/build"));
        assert_eq!(
            workspace.directories(),
            [Path::new("/w/target"), Path::new("/w/build")]
        );
    }

    #[test]
    fn a_cargo_that_prints_no_build_directory_builds_into_the_target_directory() {
        for extra in ["", ",\"build_directory\":null"] {
            let workspace = parsed(extra);
            assert_eq!(workspace.build_directory, Path::new("/w/target"));
            assert_eq!(workspace.directories(), [Path::new("/w/target")]);
        }
    }
}
