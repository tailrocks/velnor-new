// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

use velnor_model::RepoPath;
use velnor_rust::{Located, dep_info_paths, locate};

use super::{Audit, changed_since, modified};
use crate::error::CiError;
use crate::io::{SOURCE_LIMIT, SOURCE_TOTAL_LIMIT};

impl Audit<'_> {
    pub(in crate::io) fn dep_info(
        &mut self,
        workspace: &Path,
        file: &Path,
        package: Option<&str>,
    ) -> Result<Option<(RepoPath, SystemTime)>, CiError> {
        let text = read(file, &mut self.target_bytes, SOURCE_LIMIT, "dep-info file")?;
        let built = modified(file)?;
        let paths = dep_info_paths(&text);
        let Some(Located::Inside(source)) = paths
            .first()
            .map(|source| locate(self.repository, workspace, source))
        else {
            return Ok(None);
        };
        if changed_since(workspace, &paths, built) {
            return Ok(None);
        }
        for owner in self.owners.of(package, &source) {
            self.credit(&owner, workspace, &paths, false);
        }
        self.units += 1;
        Ok(Some((source, built)))
    }
}

impl Audit<'_> {
    pub(in crate::io) fn build_output_text(&mut self, file: &Path) -> Result<String, CiError> {
        read(
            file,
            &mut self.target_bytes,
            SOURCE_TOTAL_LIMIT,
            "build-script output",
        )
    }
}

fn read(file: &Path, total: &mut u64, file_limit: u64, kind: &str) -> Result<String, CiError> {
    let handle = std::fs::File::open(file)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))?;
    let remaining = SOURCE_TOTAL_LIMIT.saturating_sub(*total);
    let limit = file_limit.min(remaining);
    let mut bytes = Vec::new();
    handle
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))?;
    if bytes.len() as u64 > limit {
        let reason = if remaining < file_limit {
            format!("aggregate target data exceeds {SOURCE_TOTAL_LIMIT} bytes")
        } else {
            format!("{kind} exceeds {file_limit} bytes")
        };
        return Err(CiError::new(format!(
            "{reason} while reading {}",
            file.display()
        )));
    }
    *total += bytes.len() as u64;
    String::from_utf8(bytes)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))
}
