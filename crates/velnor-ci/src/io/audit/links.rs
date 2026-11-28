// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::REBUILD;
use crate::coverage::Fingerprint;
use crate::error::CiError;

const RUN_PREFIX: &str = "run-";
const SCRIPT_PREFIX: &str = "build-script-";

type Key = (usize, String, String);

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Unit,
    Run,
    RunUnit,
    Compiled,
}

#[derive(Default)]
pub(super) struct Links {
    wanted: BTreeMap<Key, Vec<String>>,
    provided: BTreeMap<Key, Vec<String>>,
    run_wanted: BTreeMap<Key, Vec<String>>,
    compiled: BTreeMap<(usize, String), Vec<String>>,
}

#[derive(Default)]
pub(super) struct Resolved {
    pub(super) owned: BTreeMap<Key, String>,
    pub(super) scripts: BTreeMap<Key, String>,
}

pub(super) fn kind(name: &str) -> Option<Kind> {
    match name.strip_suffix(".json") {
        Some(bare) if bare.starts_with(RUN_PREFIX) => Some(Kind::RunUnit),
        Some(bare) => Fingerprint::parse(bare).map(|_| Kind::Unit),
        None if name.contains('.') => None,
        None if name.starts_with(RUN_PREFIX) => Some(Kind::Run),
        None if name.starts_with(SCRIPT_PREFIX) => Some(Kind::Compiled),
        None => None,
    }
}

impl Links {
    pub(super) fn note(
        &mut self,
        kind: Kind,
        file: &Path,
        text: &str,
        target: usize,
        configuration: &str,
        hash: &str,
    ) -> Result<(), CiError> {
        let unit = (target, configuration.to_owned(), hash.to_owned());
        match kind {
            Kind::Run => self
                .provided
                .entry((target, configuration.to_owned(), text.trim().to_owned()))
                .or_default()
                .push(hash.to_owned()),
            Kind::Compiled => self
                .compiled
                .entry((target, text.trim().to_owned()))
                .or_default()
                .push(hash.to_owned()),
            Kind::Unit => {
                self.wanted.insert(unit, digests(file, text)?);
            }
            Kind::RunUnit => {
                self.run_wanted.insert(unit, digests(file, text)?);
            }
        }
        Ok(())
    }

    pub(super) fn resolve(self) -> Result<Resolved, CiError> {
        let Self {
            wanted,
            provided,
            run_wanted,
            compiled,
        } = self;
        let mut resolved = Resolved::default();
        for (unit, found) in wanted {
            let (target, configuration, _) = &unit;
            let runs = found
                .iter()
                .filter_map(|digest| {
                    provided.get(&(*target, configuration.clone(), digest.clone()))
                })
                .flatten();
            if let Some(run) = one(runs, &unit, "build-script runs")? {
                resolved.owned.insert(unit, run);
            }
        }
        for (run, found) in run_wanted {
            let (target, _, _) = &run;
            let scripts = found
                .iter()
                .filter_map(|digest| compiled.get(&(*target, digest.clone())))
                .flatten();
            if let Some(script) = one(scripts, &run, "compiled build scripts")? {
                resolved.scripts.insert(run, script);
            }
        }
        Ok(resolved)
    }
}

fn one<'a>(
    mut found: impl Iterator<Item = &'a String>,
    unit: &Key,
    what: &str,
) -> Result<Option<String>, CiError> {
    let Some(first) = found.next() else {
        return Ok(None);
    };
    if found.any(|other| other != first) {
        return Err(CiError::new(format!(
            "unit {} of {} depends on several {what}, so none can be matched to it; rebuild every observed target in this checkout through {REBUILD}",
            unit.2, unit.1
        )));
    }
    Ok(Some(first.clone()))
}

fn digests(file: &Path, text: &str) -> Result<Vec<String>, CiError> {
    let value = serde_json::from_str::<Value>(text).map_err(|e| {
        CiError::new(format!(
            "{} is not valid JSON ({e}), so the build-script runs it depends on cannot be matched; rebuild every observed target in this checkout through {REBUILD}",
            file.display()
        ))
    })?;
    Ok(value
        .get("deps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.as_array()?.get(3)?.as_u64())
        .map(|digest| format!("{:016x}", digest.swap_bytes()))
        .collect())
}
