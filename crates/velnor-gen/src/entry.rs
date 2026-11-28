// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RepoView};
use velnor_rust::{METADATA_PATH, metadata_file};

use crate::{GenerateError, expected_with_notes};

pub fn generated_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    generated_with_notes(view).map(|(files, _)| files)
}

pub fn generated_with_notes(
    view: &RepoView,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    let (mut files, notes) = expected_with_notes(view)?;
    files.insert(
        METADATA_PATH,
        metadata_file(view).map_err(GenerateError::Metadata)?,
    );
    Ok((files, notes))
}

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    expected_with_notes(view).map(|(files, _)| files)
}

#[must_use]
pub fn build_commit() -> &'static str {
    option_env!("VELNOR_COMMIT").unwrap_or("unknown")
}
