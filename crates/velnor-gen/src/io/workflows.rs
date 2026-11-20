// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use velnor_model::RepoPath;

use super::Error;

pub(crate) const ENGINE_CONFIG: RepoPath = RepoPath::from_static(".velnor/config.toml");

pub(crate) fn workflow_paths(root: &Path) -> Result<Vec<RepoPath>, Error> {
    let directory = root.join(".github/workflows");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::File(directory, error)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let name = entry
            .map_err(|e| Error::File(directory.clone(), e))?
            .file_name();
        let Some(name) = name.to_str() else { continue };
        let extension = Path::new(name).extension().and_then(|e| e.to_str());
        if matches!(extension, Some("yml" | "yaml")) {
            paths.push(
                RepoPath::new(format!(".github/workflows/{name}"))
                    .map_err(|e| Error::Repository(e.to_string()))?,
            );
        }
    }
    Ok(paths)
}
