// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use velnor_model::RepoPath;
use velnor_rust::{Located, dep_info_paths, locate, normalize, rerun_if_changed};

use super::super::units::UnitFile;
use super::{Audit, modified};
use crate::error::CiError;
use crate::io::{SOURCE_LIMIT, SOURCE_TOTAL_LIMIT};

impl Audit<'_> {
    pub(in crate::io) fn dep_info(
        &mut self,
        workspace: &Path,
        index: usize,
        file: &Path,
        package: Option<&str>,
    ) -> Result<Option<(RepoPath, SystemTime)>, CiError> {
        let text = read(
            file,
            &mut self.target_bytes[index],
            SOURCE_LIMIT,
            "dep-info file",
        )?;
        let built = modified(file)?;
        let paths = dep_info_paths(&text);
        let Some(source) = self.unit_source(workspace, &paths, package) else {
            return Ok(None);
        };
        if !self
            .changed_inputs(workspace, &paths, built, file, false, 1)?
            .is_empty()
        {
            if !self.owners.of(package, &source).is_empty() {
                self.stale.push(file.display().to_string());
            }
            return Ok(None);
        }
        let unseen = self.reads_unseen_output(workspace, package, &paths);
        for owner in self.owners.of(package, &source) {
            self.credit(&owner, workspace, &paths, false);
            if unseen {
                self.credit_everything(&owner);
            }
        }
        self.units += 1;
        Ok(Some((source, built)))
    }
}

impl Audit<'_> {
    pub(in crate::io) fn note_undeclared_outputs(
        &mut self,
        files: &[(UnitFile, PathBuf, Vec<usize>)],
    ) -> Result<(), CiError> {
        for (unit, output, _) in files {
            let UnitFile::BuildOutput { package, .. } = unit else {
                continue;
            };
            let mut spent = 0;
            let text = read(
                output,
                &mut spent,
                SOURCE_TOTAL_LIMIT,
                "build-script output",
            )?;
            let Some(recorded) = output
                .parent()
                .map(|directory| directory.join("root-output"))
                .and_then(|file| std::fs::read_to_string(file).ok())
            else {
                continue;
            };
            if rerun_if_changed(&text).is_empty() {
                self.undeclared
                    .insert((package.clone(), normalize(Path::new(recorded.trim()))));
            }
        }
        Ok(())
    }

    fn reads_unseen_output(
        &self,
        workspace: &Path,
        package: Option<&str>,
        paths: &[String],
    ) -> bool {
        paths.iter().any(|path| {
            let full = normalize(&workspace.join(path));
            self.undeclared.iter().any(|(owner, out)| {
                package.is_none_or(|name| name == owner) && full.starts_with(out)
            })
        })
    }

    fn unit_source(
        &self,
        workspace: &Path,
        paths: &[String],
        package: Option<&str>,
    ) -> Option<RepoPath> {
        let located: Vec<Located> = paths
            .iter()
            .map(|path| locate(self.repository, workspace, path))
            .collect();
        let named = located.iter().find_map(|path| match path {
            Located::Inside(source) if self.owners.is_target_source(package, source) => {
                Some(source.clone())
            }
            _ => None,
        });
        named.or_else(|| match located.into_iter().next() {
            Some(Located::Inside(source)) => Some(source),
            _ => None,
        })
    }

    pub(in crate::io) fn build_output_text(
        &mut self,
        index: usize,
        file: &Path,
    ) -> Result<String, CiError> {
        read(
            file,
            &mut self.target_bytes[index],
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
            format!("target data of one Cargo root exceeds {SOURCE_TOTAL_LIMIT} bytes")
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
