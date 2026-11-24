// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use velnor_model::RepoPath;
use velnor_rust::{Located, Owner, add_reads, locate, rerun_if_changed};

use super::super::units::{UnitFile, profile};
use super::Audit;
use super::freshness::Cause;
use crate::coverage::{Fingerprint, MoreInputs, RunGap, Unit, UnitKind};
use crate::error::CiError;

impl Audit<'_> {
    pub(super) fn check_stale(&self) -> Result<(), CiError> {
        if self.stale.is_empty() {
            return Ok(());
        }
        Err(CiError::new(format!(
            "found stale dep-info {}; rebuild every observed target through velnor-ci inputs build -- cargo build --workspace --all-targets or delete its stale unit files before auditing inputs",
            self.stale.join(", ")
        )))
    }

    pub(super) fn record(
        &mut self,
        indices: &[usize],
        file: &Path,
        unit: UnitFile,
    ) -> Result<(), CiError> {
        match unit {
            UnitFile::Fingerprint {
                configuration,
                package,
                hash,
                name,
            } => {
                for index in self.member_owners(&package, indices) {
                    self.fingerprint(
                        index,
                        configuration.clone(),
                        package.clone(),
                        hash.clone(),
                        &name,
                    );
                }
            }
            UnitFile::DepInfo {
                configuration,
                hash,
                stem,
            } => self.record_dep_info(indices, file, &configuration, hash.as_deref(), &stem)?,
            UnitFile::Script { profile, package } => {
                let owners = self.member_owners(&package, indices);
                if let Some(first) = owners.first() {
                    let base = self.roots[*first].clone();
                    self.script(&base, &owners, file, &profile, &package)?;
                }
            }
            UnitFile::BuildOutput {
                configuration,
                package,
                hash,
            } => {
                let owners = self.member_owners(&package, indices);
                if !owners.is_empty() {
                    self.build_output(&owners, file, &configuration, &package, &hash)?;
                }
            }
            UnitFile::Directory { name, root } => {
                for index in indices {
                    self.directory(*index, name.clone(), root);
                }
            }
        }
        Ok(())
    }

    fn record_dep_info(
        &mut self,
        indices: &[usize],
        file: &Path,
        configuration: &str,
        hash: Option<&str>,
        stem: &str,
    ) -> Result<(), CiError> {
        let mut hits: Vec<(usize, (String, Fingerprint))> = Vec::new();
        for index in indices {
            let key = (
                *index,
                configuration.to_owned(),
                hash.unwrap_or(stem).to_owned(),
            );
            self.present.insert(key.clone());
            let identity = match hash {
                Some(_) => self.fingerprints.get(&key),
                None => self.names.get(&key),
            };
            if let Some(identity) = identity {
                hits.push((*index, identity.clone()));
            }
        }
        if hash.is_none() && hits.is_empty() {
            return Ok(());
        }
        let budget = hits.first().map_or(indices[0], |(index, _)| *index);
        let base = self.roots[budget].clone();
        let package = hits.first().map(|(_, (package, _))| package.as_str());
        let Some((source, _)) = self.dep_info(&base, budget, file, package)? else {
            return Ok(());
        };
        for (index, (package, fingerprint)) in hits {
            self.built[index].units.insert(Unit {
                configuration: configuration.to_owned(),
                kind: fingerprint.kind,
                package,
                target: fingerprint.target,
                source: source.clone(),
            });
        }
        Ok(())
    }

    fn member_owners(&self, package: &str, indices: &[usize]) -> Vec<usize> {
        match self.members.get(package) {
            Some(owners) => indices
                .iter()
                .copied()
                .filter(|index| owners.contains(index))
                .collect(),
            None => indices.to_vec(),
        }
    }

    fn build_output(
        &mut self,
        indices: &[usize],
        file: &Path,
        configuration: &str,
        package: &str,
        hash: &str,
    ) -> Result<(), CiError> {
        let text = self.build_output_text(indices[0], file)?;
        let built = modified(file)?;
        let paths = rerun_if_changed(&text);
        let mut seen = BTreeSet::new();
        for &index in indices {
            let known: Vec<(RepoPath, SystemTime)> = self
                .scripts
                .get(&(index, profile(configuration).to_owned(), package.to_owned()))
                .into_iter()
                .flatten()
                .map(|(source, compiled)| (source.clone(), *compiled))
                .collect();
            let scripts: Vec<RepoPath> = known
                .iter()
                .filter(|(_, compiled)| *compiled <= built)
                .map(|(source, _)| source.clone())
                .collect();
            let mut gap = if known.is_empty() {
                Some(RunGap::Uncompiled)
            } else {
                scripts.is_empty().then_some(RunGap::Older)
            };
            let mut current = false;
            for source in scripts {
                let mut ran = false;
                for owner in self.owners.of(Some(package), &source) {
                    let base = owner.directory().map_or_else(
                        || self.repository.to_path_buf(),
                        |d| self.repository.join(d.as_str()),
                    );
                    if let Some((first, cause)) = self
                        .changed_inputs(&base, &paths, built, file, true, 1)?
                        .into_iter()
                        .next()
                    {
                        if gap.is_none() {
                            gap = Some(self.stale_gap(&base, &paths, built, file, first, cause));
                        }
                        continue;
                    }
                    ran = true;
                    if seen.insert((owner.manifest().clone(), owner.directory().cloned())) {
                        self.credit(&owner, &base, &paths, true);
                        self.units += 1;
                    }
                }
                if ran {
                    current = true;
                    self.built[index].units.insert(Unit {
                        configuration: configuration.to_owned(),
                        kind: UnitKind::Run,
                        package: package.to_owned(),
                        target: String::new(),
                        source,
                    });
                }
            }
            if current {
                self.present
                    .insert((index, configuration.to_owned(), hash.to_owned()));
            } else if let Some(gap) = gap {
                self.built[index].run_gaps.insert(
                    (
                        configuration.to_owned(),
                        package.to_owned(),
                        hash.to_owned(),
                    ),
                    gap,
                );
            }
        }
        Ok(())
    }

    fn stale_gap(
        &mut self,
        base: &Path,
        paths: &[String],
        built: SystemTime,
        output: &Path,
        first: String,
        cause: Cause,
    ) -> RunGap {
        std::mem::swap(&mut self.freshness, &mut self.diagnostic);
        let all = self.changed_inputs(base, paths, built, output, true, usize::MAX);
        std::mem::swap(&mut self.freshness, &mut self.diagnostic);
        let more = all.map_or(MoreInputs::default(), |all| {
            all.iter()
                .skip(1)
                .fold(MoreInputs::default(), |mut more, (_, cause)| {
                    match cause {
                        Cause::Changed => more.changed += 1,
                        Cause::Unproven => more.unproven += 1,
                    }
                    more
                })
        });
        match cause {
            Cause::Changed => RunGap::Stale { input: first, more },
            Cause::Unproven => RunGap::Unproven { input: first, more },
        }
    }

    pub(super) fn credit(
        &mut self,
        owner: &Owner<'_>,
        base: &Path,
        paths: &[String],
        declared: bool,
    ) {
        let mut reads = Vec::new();
        for path in paths {
            match locate(self.repository, base, path) {
                Located::Inside(read) => reads.push(Some(read)),
                Located::Repository => reads.push(None),
                Located::Outside => {}
            }
            if let Ok(Located::Inside(target)) =
                fs::canonicalize(base.join(path)).map(|target| locate(self.repository, &target, ""))
            {
                reads.push(Some(target));
            }
        }
        add_reads(
            &mut self.found,
            self.tracked,
            owner,
            &reads,
            declared,
            &self.targets,
        );
    }
}

pub(super) fn modified(path: &Path) -> Result<SystemTime, CiError> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))
}
