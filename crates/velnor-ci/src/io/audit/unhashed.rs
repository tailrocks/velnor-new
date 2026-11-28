// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::mem;
use std::path::PathBuf;

use super::{Audit, REBUILD};
use crate::coverage::UnitKind;
use crate::error::CiError;

pub(in crate::io) struct Pending {
    pub(super) indices: Vec<usize>,
    pub(super) file: PathBuf,
    pub(super) configuration: String,
    pub(super) stem: String,
}

impl Audit<'_> {
    pub(in crate::io) fn record_unhashed(&mut self) -> Result<(), CiError> {
        for pending in mem::take(&mut self.unhashed) {
            let hash = self.hash_of(&pending)?;
            self.record_dep_info(
                &pending.indices,
                &pending.file,
                &pending.configuration,
                &hash,
            )?;
        }
        Ok(())
    }

    fn hash_of(&self, pending: &Pending) -> Result<String, CiError> {
        let candidates: BTreeSet<&String> = self
            .fingerprints
            .iter()
            .map(|(key, (_, fingerprint))| (key, fingerprint))
            .chain(
                self.foreign
                    .iter()
                    .map(|(key, (_, fingerprint))| (key, fingerprint)),
            )
            .filter(|((index, configuration, _), fingerprint)| {
                pending.indices.contains(index)
                    && *configuration == pending.configuration
                    && fingerprint.kind == UnitKind::Normal
                    && fingerprint.target == pending.stem
            })
            .filter(|(key, _)| !self.present.contains(*key))
            .map(|((_, _, hash), _)| hash)
            .collect();
        let mut found = candidates.iter();
        match (found.next(), found.next()) {
            (Some(hash), None) => Ok((*hash).clone()),
            _ => Err(CiError::new(format!(
                "{} carries no unit hash, and {} fingerprint units of target {} have no hashed dep-info, so no single fingerprint can be matched to it; rebuild every observed target in this checkout through {REBUILD}",
                pending.file.display(),
                candidates.len(),
                pending.stem
            ))),
        }
    }
}
