// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::error::Reason;

const KEY: &str = "external_workflows";
const DIRECTORY: &str = ".github/workflows/";

/// Parses `external_workflows`: each entry names a file directly under `.github/workflows/`
/// with a `.yml` or `.yaml` extension, once. The generator checks that the file exists and
/// that velnor-gen does not write it.
pub(crate) fn parse(values: Option<Vec<String>>) -> Result<Option<Vec<RepoPath>>, Reason> {
    let Some(values) = values else {
        return Ok(None);
    };
    let mut paths: Vec<RepoPath> = Vec::with_capacity(values.len());
    for value in values {
        let refused = || Reason::InvalidValue {
            key: KEY,
            value: value.clone(),
        };
        let name = value.strip_prefix(DIRECTORY).ok_or_else(refused)?;
        let directly_inside = !name.contains('/');
        let yaml = matches!(
            std::path::Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("yml" | "yaml")
        );
        if !directly_inside || !yaml {
            return Err(refused());
        }
        let path = RepoPath::new(value.as_str()).map_err(|_| refused())?;
        if paths.contains(&path) {
            return Err(refused());
        }
        paths.push(path);
    }
    Ok(Some(paths))
}
