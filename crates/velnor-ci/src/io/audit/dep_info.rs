// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use velnor_model::RepoPath;
use velnor_rust::{Located, dep_info_paths, locate, normalize, rerun_if_changed};

use super::super::units::UnitFile;
use super::{Audit, REBUILD, modified};
use crate::error::CiError;
use crate::io::{SOURCE_LIMIT, SOURCE_TOTAL_LIMIT};

const ROOT_OUTPUT_LIMIT: u64 = 8 << 10;

struct Run {
    out: Option<PathBuf>,
    undeclared: bool,
}

#[derive(Default)]
pub(super) struct BuildOutputs {
    runs: BTreeMap<(usize, String), Vec<Run>>,
    texts: BTreeMap<PathBuf, String>,
    read_only: OnceCell<Vec<PathBuf>>,
}

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
        self.refuse_unexplained_reads(workspace, index, package, file, &paths)?;
        let unseen = self.reads_unseen_output(workspace, index, package, &paths);
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
    pub(in crate::io) fn note_build_outputs(
        &mut self,
        files: &[(UnitFile, PathBuf, Vec<usize>)],
    ) -> Result<(), CiError> {
        for (unit, output, indices) in files {
            let (UnitFile::BuildOutput { package, .. }, Some(&budget)) = (unit, indices.first())
            else {
                continue;
            };
            let index = self
                .member_owners(package, indices)
                .first()
                .copied()
                .unwrap_or(budget);
            let text = read(
                output,
                &mut self.target_bytes[budget],
                SOURCE_TOTAL_LIMIT,
                "build-script output",
            )?;
            let out = self.recorded_root(output, budget)?;
            self.outputs
                .runs
                .entry((index, package.clone()))
                .or_default()
                .push(Run {
                    out,
                    undeclared: rerun_if_changed(&text).is_empty(),
                });
            self.outputs.texts.insert(output.clone(), text);
        }
        Ok(())
    }

    fn recorded_root(&mut self, output: &Path, index: usize) -> Result<Option<PathBuf>, CiError> {
        let Some(file) = output
            .parent()
            .map(|directory| directory.join("root-output"))
            .filter(|file| file.is_file())
        else {
            return Ok(None);
        };
        let text = read(
            &file,
            &mut self.target_bytes[index],
            ROOT_OUTPUT_LIMIT,
            "root-output file",
        )?;
        let path = normalize(Path::new(text.trim()));
        Ok(own_out(output, &path).then_some(path))
    }

    fn reads_unseen_output(
        &self,
        workspace: &Path,
        index: usize,
        package: Option<&str>,
        paths: &[String],
    ) -> bool {
        self.runs_of(index, package)
            .filter(|run| run.undeclared)
            .any(|run| match &run.out {
                Some(out) => paths
                    .iter()
                    .any(|path| normalize(&workspace.join(path)).starts_with(out)),
                None => true,
            })
    }

    fn runs_of(&self, index: usize, package: Option<&str>) -> impl Iterator<Item = &Run> {
        package
            .and_then(|name| self.outputs.runs.get(&(index, name.to_owned())))
            .into_iter()
            .flatten()
    }

    fn refuse_unexplained_reads(
        &self,
        workspace: &Path,
        index: usize,
        package: Option<&str>,
        file: &Path,
        paths: &[String],
    ) -> Result<(), CiError> {
        let outside: Vec<PathBuf> = paths
            .iter()
            .filter(|path| matches!(locate(self.repository, workspace, path), Located::Outside))
            .map(|path| normalize(&workspace.join(path)))
            .filter(|full| {
                !self
                    .runs_of(index, package)
                    .filter_map(|run| run.out.as_ref())
                    .any(|out| full.starts_with(out))
            })
            .collect();
        if outside.is_empty() {
            return Ok(());
        }
        let roots = self.outputs.read_only.get_or_init(read_only_roots);
        let Some(first) = outside
            .iter()
            .find(|full| !roots.iter().any(|root| full.starts_with(root)))
        else {
            return Ok(());
        };
        Err(CiError::new(format!(
            "{} names {}, which is outside the repository, in no build-script output directory and in no read-only source root (the CARGO_HOME registry or git folders, the Rust sysroot); the target was built in another environment or a build reads a file this audit cannot see: rebuild every observed target in this checkout through {REBUILD}",
            file.display(),
            first.display()
        )))
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
        if let Some(text) = self.outputs.texts.remove(file) {
            return Ok(text);
        }
        read(
            file,
            &mut self.target_bytes[index],
            SOURCE_TOTAL_LIMIT,
            "build-script output",
        )
    }
}

fn own_out(output: &Path, recorded: &Path) -> bool {
    let unit = match output.file_name().and_then(|name| name.to_str()) {
        Some("stdout") => output.parent().and_then(Path::parent),
        _ => output.parent(),
    };
    let Some((unit, parent)) = unit.zip(recorded.parent()) else {
        return false;
    };
    recorded.is_absolute()
        && recorded.file_name().is_some_and(|name| name == "out")
        && (normalize(unit) == parent
            || std::fs::canonicalize(unit)
                .ok()
                .zip(std::fs::canonicalize(parent).ok())
                .is_some_and(|(unit, parent)| unit == parent))
}

fn trusted(root: &Path) -> bool {
    root.is_absolute() && root.parent().is_some()
}

fn read_only_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    let mut roots: Vec<PathBuf> = home
        .filter(|home| trusted(home))
        .into_iter()
        .flat_map(|home| [home.join("registry"), home.join("git")])
        .collect();
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    if let Ok(output) = Command::new(compiler).args(["--print", "sysroot"]).output()
        && output.status.success()
    {
        roots.push(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim(),
        ));
    }
    let resolved: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect();
    roots.extend(resolved);
    roots.retain(|root| trusted(root));
    roots
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
