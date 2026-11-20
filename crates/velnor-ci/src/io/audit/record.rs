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
use crate::coverage::{Fingerprint, Unit, UnitKind};
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
            } => self.record_dep_info(
                indices,
                file,
                configuration.as_deref(),
                hash.as_deref(),
                &stem,
            )?,
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
        configuration: Option<&str>,
        hash: Option<&str>,
        stem: &str,
    ) -> Result<(), CiError> {
        let mut hits: Vec<(usize, (String, Fingerprint))> = Vec::new();
        for index in indices {
            let key = configuration.map(|configuration| {
                let key = (
                    *index,
                    configuration.to_owned(),
                    hash.unwrap_or(stem).to_owned(),
                );
                self.present.insert(key.clone());
                key
            });
            if let Some(identity) = key.and_then(|key| match hash {
                Some(_) => self.fingerprints.get(&key),
                None => self.names.get(&key),
            }) {
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
        if let Some(configuration) = configuration {
            for (index, (package, fingerprint)) in hits {
                self.built[index].units.insert(Unit {
                    configuration: configuration.to_owned(),
                    kind: fingerprint.kind,
                    package,
                    target: fingerprint.target,
                    source: source.clone(),
                });
            }
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
            let scripts: Vec<RepoPath> = self
                .scripts
                .get(&(index, profile(configuration).to_owned(), package.to_owned()))
                .into_iter()
                .flatten()
                .filter(|(_, compiled)| **compiled <= built)
                .map(|(source, _)| source.clone())
                .collect();
            let mut current = false;
            for source in scripts {
                let mut ran = false;
                for owner in self.owners.of(Some(package), &source) {
                    let base = owner.directory().map_or_else(
                        || self.repository.to_path_buf(),
                        |d| self.repository.join(d.as_str()),
                    );
                    if self.changed_since(&base, &paths, built, file, true)? {
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
            }
        }
        Ok(())
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
