// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::RepoPath;

use super::{File, MetadataFileError, path_of};

pub fn recorded_roots(recorded: Option<&[u8]>) -> Result<Vec<RepoPath>, MetadataFileError> {
    let bytes = recorded.ok_or(MetadataFileError::Missing)?;
    let file: File = serde_json::from_slice(bytes)
        .map_err(|error| MetadataFileError::Unreadable(error.to_string()))?;
    file.roots.keys().map(|root| path_of(root)).collect()
}

pub fn recorded_members(
    recorded: Option<&[u8]>,
) -> Result<BTreeMap<RepoPath, BTreeSet<String>>, MetadataFileError> {
    let bytes = recorded.ok_or(MetadataFileError::Missing)?;
    let file: File = serde_json::from_slice(bytes)
        .map_err(|error| MetadataFileError::Unreadable(error.to_string()))?;
    file.roots
        .into_iter()
        .map(|(root, entry)| {
            Ok((
                path_of(&root)?,
                entry
                    .members
                    .into_iter()
                    .map(|member| member.name)
                    .collect(),
            ))
        })
        .collect()
}

pub fn recorded_target_dirs(
    recorded: Option<&[u8]>,
) -> Result<BTreeMap<RepoPath, RepoPath>, MetadataFileError> {
    let bytes = recorded.ok_or(MetadataFileError::Missing)?;
    let file: File = serde_json::from_slice(bytes)
        .map_err(|error| MetadataFileError::Unreadable(error.to_string()))?;
    file.roots
        .iter()
        .filter_map(|(root, record)| Some((root, record.target_dir.as_ref()?)))
        .map(|(root, dir)| Ok((path_of(root)?, path_of(dir)?)))
        .collect()
}
