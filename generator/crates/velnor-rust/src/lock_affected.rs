// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::PackageName;

use crate::cargo_lock::CargoLock;
use crate::lock::LockError;

const TRUSTED_SOURCES: [&str; 2] = ["registry+", "sparse+"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Member(String),
    Path(String, String),
    Locked {
        name: String,
        version: String,
        source: String,
        checksum: Option<String>,
    },
}

impl Key {
    fn name(&self) -> &str {
        match self {
            Self::Member(name) | Self::Path(name, _) | Self::Locked { name, .. } => name,
        }
    }

    fn untrusted(&self) -> bool {
        matches!(self, Self::Locked { source, .. }
            if !TRUSTED_SOURCES.iter().any(|scheme| source.starts_with(scheme)))
    }

    fn has_source(&self) -> bool {
        matches!(self, Self::Locked { .. })
    }
}

struct Index {
    keys: Vec<Key>,
    by_key: BTreeMap<Key, usize>,
    dependencies: Vec<BTreeSet<Key>>,
    dependents: Vec<Vec<usize>>,
}

impl Index {
    fn new(lock: &CargoLock, members: &BTreeSet<&str>) -> Result<Self, LockError> {
        let keys: Vec<Key> = lock
            .packages
            .iter()
            .map(|package| match &package.source {
                None if members.contains(package.name.as_str()) => {
                    Key::Member(package.name.clone())
                }
                None => Key::Path(package.name.clone(), package.version.clone()),
                Some(source) => Key::Locked {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    source: source.clone(),
                    checksum: package.checksum.clone(),
                },
            })
            .collect();
        let mut by_key = BTreeMap::new();
        for (index, key) in keys.iter().enumerate() {
            if by_key.insert(key.clone(), index).is_some() {
                return Err(LockError::new(format!(
                    "lockfile lists {} twice",
                    key.name()
                )));
            }
        }
        let dependencies = lock
            .packages
            .iter()
            .map(|package| {
                package
                    .dependencies
                    .iter()
                    .map(|dependency| keys[*dependency].clone())
                    .collect()
            })
            .collect();
        let mut dependents = vec![Vec::new(); keys.len()];
        for (index, package) in lock.packages.iter().enumerate() {
            for dependency in &package.dependencies {
                dependents[*dependency].push(index);
            }
        }
        Ok(Self {
            keys,
            by_key,
            dependencies,
            dependents,
        })
    }

    fn without_source(&self) -> BTreeSet<&Key> {
        self.keys.iter().filter(|key| !key.has_source()).collect()
    }
}

pub fn lock_affected(
    base: &str,
    head: &str,
    members: &[PackageName],
) -> Result<BTreeMap<PackageName, String>, LockError> {
    let (base, head) = (CargoLock::parse(base)?, CargoLock::parse(head)?);
    if base.format != head.format {
        return Err(LockError::new("the lockfile format differs"));
    }
    if base.patch != head.patch {
        return Err(LockError::new("the [[patch]] entries differ"));
    }
    let names: BTreeSet<&str> = members.iter().map(PackageName::as_str).collect();
    let (base, head) = (Index::new(&base, &names)?, Index::new(&head, &names)?);
    if base.without_source() != head.without_source() {
        return Err(LockError::new(
            "a package without a source was added, removed or renamed",
        ));
    }
    if let Some(missing) = members
        .iter()
        .find(|member| !head.by_key.contains_key(&Key::Member(member.to_string())))
    {
        return Err(LockError::new(format!(
            "member {missing} is not in the lockfile"
        )));
    }
    let changed = changed(&base, &head)?;
    Ok(reached(&head, &changed, members))
}

fn changed(base: &Index, head: &Index) -> Result<Vec<usize>, LockError> {
    let gone = base
        .keys
        .iter()
        .filter(|key| !head.by_key.contains_key(key));
    let added: Vec<usize> = (0..head.keys.len())
        .filter(|index| !base.by_key.contains_key(&head.keys[*index]))
        .collect();
    if let Some(key) = gone
        .chain(added.iter().map(|index| &head.keys[*index]))
        .find(|key| key.untrusted())
    {
        return Err(LockError::new(format!(
            "{} comes from a source other than a registry",
            key.name()
        )));
    }
    let rewired = (0..head.keys.len()).filter(|index| {
        base.by_key
            .get(&head.keys[*index])
            .is_some_and(|before| base.dependencies[*before] != head.dependencies[*index])
    });
    Ok(added.iter().copied().chain(rewired).collect())
}

fn reached(
    head: &Index,
    changed: &[usize],
    members: &[PackageName],
) -> BTreeMap<PackageName, String> {
    let by_name: BTreeMap<&str, &PackageName> = members
        .iter()
        .map(|member| (member.as_str(), member))
        .collect();
    let mut found = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for origin in changed {
        let cause = head.keys[*origin].name();
        let mut pending = vec![*origin];
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            if let Key::Member(name) = &head.keys[index]
                && let Some(member) = by_name.get(name.as_str())
            {
                found.insert((*member).clone(), cause.to_owned());
            }
            pending.extend(&head.dependents[index]);
        }
    }
    found
}
