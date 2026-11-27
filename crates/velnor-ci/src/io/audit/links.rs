// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;

use crate::coverage::Fingerprint;

const RUN_PREFIX: &str = "run-";
const SCRIPT: &str = "build_script_build";

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Unit,
    Run,
}

#[derive(Default)]
pub(super) struct Links {
    wanted: BTreeMap<(usize, String, String), String>,
    provided: BTreeMap<(String, String), String>,
}

pub(super) fn kind(name: &str) -> Option<Kind> {
    if name.starts_with(RUN_PREFIX) && !name.contains('.') {
        return Some(Kind::Run);
    }
    name.strip_suffix(".json")
        .and_then(Fingerprint::parse)
        .map(|_| Kind::Unit)
}

impl Links {
    pub(super) fn note(
        &mut self,
        kind: Kind,
        text: &str,
        configuration: &str,
        hash: &str,
        indices: &[usize],
    ) {
        match kind {
            Kind::Run => {
                self.provided.insert(
                    (configuration.to_owned(), text.trim().to_owned()),
                    hash.to_owned(),
                );
            }
            Kind::Unit => {
                if let Some(run) = dependency(text) {
                    for index in indices {
                        self.wanted.insert(
                            (*index, configuration.to_owned(), hash.to_owned()),
                            run.clone(),
                        );
                    }
                }
            }
        }
    }

    pub(super) fn resolve(self) -> BTreeMap<(usize, String, String), String> {
        let Self { wanted, provided } = self;
        wanted
            .into_iter()
            .filter_map(|((index, configuration, unit), digest)| {
                let run = provided.get(&(configuration.clone(), digest))?.clone();
                Some(((index, configuration, unit), run))
            })
            .collect()
    }
}

fn dependency(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value.get("deps")?.as_array()?.iter().find_map(|entry| {
        let entry = entry.as_array()?;
        (entry.get(1)?.as_str()? == SCRIPT)
            .then(|| entry.get(3)?.as_u64())
            .flatten()
            .map(|digest| format!("{:016x}", digest.swap_bytes()))
    })
}
