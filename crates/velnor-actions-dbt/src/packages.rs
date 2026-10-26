// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::DbtError;
use crate::paths::stays_inside;
use crate::yaml::top_entries;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSource {
    Hub(String),

    Git(String),

    Local(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub source: PackageSource,

    pub pin: Option<String>,
}

impl Package {
    #[must_use]
    pub fn id(&self) -> &str {
        match &self.source {
            PackageSource::Hub(id) | PackageSource::Git(id) | PackageSource::Local(id) => id,
        }
    }
}

fn to_package(entry: &BTreeMap<String, String>) -> Result<Package, DbtError> {
    if let Some(id) = entry.get("package") {
        return Ok(Package {
            source: PackageSource::Hub(id.clone()),
            pin: entry.get("version").cloned(),
        });
    }
    if let Some(url) = entry.get("git") {
        return Ok(Package {
            source: PackageSource::Git(url.clone()),
            pin: entry.get("revision").cloned(),
        });
    }
    if let Some(dir) = entry.get("local") {
        return Ok(Package {
            source: PackageSource::Local(dir.clone()),
            pin: None,
        });
    }
    let kind = if entry.contains_key("tarball") {
        "tarball"
    } else {
        entry.keys().next().map_or("empty", String::as_str)
    };
    Err(DbtError::new("dbt_package_source_unsupported", kind))
}

pub fn parse_packages(text: &str) -> Result<Vec<Package>, DbtError> {
    let entries = top_entries(text, "packages")
        .map_err(|line| DbtError::new("dbt_packages_unparsed", &line))?
        .unwrap_or_default();
    entries.iter().map(to_package).collect()
}

fn is_sha1_revision(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_exact_hub_version(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

#[must_use]
pub fn verify_lock(manifest: &[Package], lock: Option<&[Package]>) -> Vec<DbtError> {
    let mut findings = Vec::new();
    for package in manifest {
        if let PackageSource::Local(dir) = &package.source
            && !stays_inside(dir)
        {
            findings.push(DbtError::new("dbt_local_package_escapes_project", dir));
        }
    }
    let remote: Vec<&Package> = manifest
        .iter()
        .filter(|package| !matches!(package.source, PackageSource::Local(_)))
        .collect();
    if remote.is_empty() {
        findings.sort();
        return findings;
    }
    let Some(lock) = lock else {
        findings.push(DbtError::bare("dbt_package_lock_missing"));
        return findings;
    };
    for package in remote {
        let Some(locked) = lock.iter().find(|l| l.source == package.source) else {
            findings.push(DbtError::new("dbt_package_not_locked", package.id()));
            continue;
        };
        let pinned = locked
            .pin
            .as_deref()
            .is_some_and(|pin| match locked.source {
                PackageSource::Git(_) => is_sha1_revision(pin),
                _ => is_exact_hub_version(pin),
            });
        if !pinned {
            findings.push(DbtError::new("dbt_package_lock_unpinned", package.id()));
        }
    }
    findings.sort();
    findings
}
