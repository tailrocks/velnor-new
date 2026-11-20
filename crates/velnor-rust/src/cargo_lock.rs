// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::lock::LockError;

const SUPPORTED_FORMATS: [i64; 2] = [3, 4];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: Option<toml::Value>,
    #[serde(default)]
    package: Vec<Entry>,
    patch: Option<toml::Value>,
    metadata: Option<toml::Value>,
    root: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct CargoLock {
    pub(crate) format: i64,
    pub(crate) patch: Option<toml::Value>,
    pub(crate) packages: Vec<Package>,
}

#[derive(Debug)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: Option<String>,
    pub(crate) checksum: Option<String>,
    pub(crate) dependencies: Vec<usize>,
}

impl CargoLock {
    pub(crate) fn parse(text: &str) -> Result<Self, LockError> {
        let document: Document =
            toml::from_str(text).map_err(|error| LockError::new(error.message().to_owned()))?;
        let format = match document.version {
            Some(toml::Value::Integer(format)) if SUPPORTED_FORMATS.contains(&format) => format,
            other => {
                return Err(LockError::new(format!(
                    "lockfile format {other:?} is not one of {SUPPORTED_FORMATS:?}"
                )));
            }
        };
        if document.metadata.is_some() || document.root.is_some() {
            return Err(LockError::new("lockfile has a [metadata] or [root] table"));
        }
        let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (index, entry) in document.package.iter().enumerate() {
            by_name.entry(&entry.name).or_default().push(index);
        }
        let packages = document
            .package
            .iter()
            .map(|entry| {
                Ok(Package {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    source: entry.source.clone(),
                    checksum: entry.checksum.clone(),
                    dependencies: entry
                        .dependencies
                        .iter()
                        .map(|reference| resolve(&document.package, &by_name, reference))
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<_, LockError>>()?;
        Ok(Self {
            format,
            patch: document.patch,
            packages,
        })
    }
}

fn resolve(
    entries: &[Entry],
    by_name: &BTreeMap<&str, Vec<usize>>,
    reference: &str,
) -> Result<usize, LockError> {
    let mut parts = reference.splitn(3, ' ');
    let name = parts.next().unwrap_or_default();
    let version = parts.next();
    let source = parts
        .next()
        .map(|source| source.trim_start_matches('(').trim_end_matches(')'));
    let mut found = by_name
        .get(name)
        .into_iter()
        .flatten()
        .copied()
        .filter(|index| {
            let entry = &entries[*index];
            version.is_none_or(|version| entry.version == version)
                && source.is_none_or(|source| entry.source.as_deref() == Some(source))
        });
    match (found.next(), found.next()) {
        (Some(index), None) => Ok(index),
        _ => Err(LockError::new(format!(
            "dependency {reference:?} does not name exactly one package"
        ))),
    }
}
