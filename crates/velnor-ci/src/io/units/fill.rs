// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_model::PackageName;

use crate::coverage::{Fingerprint, HOST, UnitKind};
use crate::error::CiError;
use crate::io::audit::Audit;

impl Audit<'_> {
    pub(in crate::io) fn fingerprint(
        &mut self,
        index: usize,
        configuration: String,
        package: String,
        hash: String,
        name: &str,
    ) {
        if let Some(fingerprint) = Fingerprint::parse(name) {
            if fingerprint.kind == UnitKind::Normal {
                self.names.insert(
                    (index, configuration.clone(), fingerprint.target.clone()),
                    (package.clone(), fingerprint.clone()),
                );
            }
            self.fingerprints.insert(
                (index, configuration.clone(), hash.clone()),
                (package.clone(), fingerprint),
            );
        }
        if let Ok(package) = PackageName::new(package) {
            if name.starts_with("lib-") {
                self.built[index].libraries.insert((configuration, package));
            } else if name.starts_with("build-script-") && !name.contains('.') {
                self.built[index].builders.insert((configuration, package));
            } else if name.starts_with("run-") && !name.contains('.') {
                self.runs
                    .insert((index, configuration, hash), package.to_string());
            }
        }
    }

    pub(in crate::io) fn directory(&mut self, index: usize, name: String, root: bool) {
        if root {
            self.rooted.insert((index, name));
        } else {
            self.placed.insert((index, name));
        }
    }

    pub(in crate::io) fn script(
        &mut self,
        workspace: &Path,
        indices: &[usize],
        file: &Path,
        profile: &str,
        package: &str,
    ) -> Result<(), CiError> {
        let Some((source, built)) = self.dep_info(workspace, indices[0], file, Some(package))?
        else {
            return Ok(());
        };
        for index in indices {
            if profile == HOST {
                self.built[*index]
                    .scripts
                    .insert((package.to_owned(), source.clone()));
            }
            self.scripts
                .entry((*index, profile.to_owned(), package.to_owned()))
                .or_default()
                .entry(source.clone())
                .and_modify(|compiled| *compiled = (*compiled).min(built))
                .or_insert(built);
        }
        Ok(())
    }

    pub(in crate::io) fn orphans(&mut self) {
        for ((index, configuration, hash), (package, fingerprint)) in &self.fingerprints {
            let key = |name: &String| (*index, configuration.clone(), name.clone());
            if !self.present.contains(&key(hash))
                && !self.present.contains(&key(&fingerprint.target))
            {
                self.built[*index].orphans.insert((
                    configuration.clone(),
                    package.clone(),
                    fingerprint.clone(),
                ));
            }
        }
        for ((index, configuration, hash), package) in &self.runs {
            let key = (*index, configuration.clone(), hash.clone());
            if !self.present.contains(&key) {
                self.built[*index].orphans.insert((
                    configuration.clone(),
                    package.clone(),
                    Fingerprint {
                        kind: UnitKind::Run,
                        target: String::new(),
                    },
                ));
            }
        }
        for (index, name) in &self.placed {
            if !self.rooted.contains(&(*index, name.clone())) {
                self.built[*index].placed.insert(name.clone());
            }
        }
    }
}
