// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use crate::exec::ExecError;

async fn resolve(path: &Path) -> Result<PathBuf, ExecError> {
    tokio::fs::canonicalize(path)
        .await
        .map_err(|source| ExecError::io(format!("resolve PATH entry {}", path.display()), source))
}

pub(super) async fn require_confined(
    entries: &[String],
    directories: &[(&str, &Path)],
) -> Result<(), ExecError> {
    let mut real = Vec::with_capacity(directories.len());
    for (_, directory) in directories {
        real.push(resolve(directory).await?);
    }
    for entry in entries {
        let path = Path::new(entry);
        let in_store = directories
            .iter()
            .map(|(_, directory)| *directory)
            .chain(real.iter().map(PathBuf::as_path))
            .any(|directory| path.starts_with(directory));
        if !in_store {
            continue;
        }
        let resolved = resolve(path).await?;
        if !real.iter().any(|directory| resolved.starts_with(directory)) {
            return Err(ExecError::Invalid(format!(
                "mise policy: PATH entry {entry} resolves to {} outside the read-only tools store",
                resolved.display()
            )));
        }
    }
    Ok(())
}
