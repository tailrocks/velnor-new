// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::super::{CollectorError, filesystem};
use super::{FIXTURES, Mode, golden_manifest};

pub(super) enum Snapshot {
    Absent,
    Existing {
        cases: BTreeMap<String, filesystem::Tree>,
        tree: filesystem::Tree,
    },
}

impl Snapshot {
    pub(super) fn case(&self, name: &str) -> Result<&filesystem::Tree, CollectorError> {
        let Self::Existing { cases, .. } = self else {
            return Err(CollectorError::mismatch(
                "golden cases directory is missing",
            ));
        };
        cases
            .get(name)
            .ok_or_else(|| CollectorError::operational("unknown golden case"))
    }
}

pub(super) fn snapshot(golden_dir: &Path, mode: Mode) -> Result<Snapshot, CollectorError> {
    match std::fs::symlink_metadata(golden_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return match mode {
                Mode::Capture => Ok(Snapshot::Absent),
                Mode::Check => Err(CollectorError::mismatch(
                    "golden cases directory is missing",
                )),
            };
        }
        Err(error) => return Err(CollectorError::operational(error)),
        Ok(_) => {}
    }
    let tree = filesystem::read_tree(golden_dir)?;
    match tree.get("cases") {
        Some(filesystem::Entry::Directory) => {}
        Some(_) => return Err(CollectorError::mismatch("golden cases directory changed")),
        None => {
            return Err(CollectorError::mismatch(
                "golden cases directory is missing",
            ));
        }
    }
    let inventory = case_tree("cases", &tree);
    validate_case_inventory(&inventory)?;
    let manifest = match tree.get("MANIFEST.sha256") {
        Some(filesystem::Entry::File { bytes, .. }) => bytes,
        Some(_) => {
            return Err(CollectorError::operational(
                "golden manifest must be a regular non-symlink file",
            ));
        }
        None => return Err(CollectorError::mismatch("golden manifest is missing")),
    };
    golden_manifest::verify(manifest, &inventory)?;
    let cases = case_trees(&inventory);
    Ok(Snapshot::Existing { cases, tree })
}

pub(super) fn verify_unchanged(
    golden_dir: &Path,
    expected: &Snapshot,
) -> Result<(), CollectorError> {
    let Snapshot::Existing {
        tree: expected_tree,
        ..
    } = expected
    else {
        return match std::fs::symlink_metadata(golden_dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(changed()),
        };
    };
    let tree = filesystem::read_tree(golden_dir).map_err(|_| changed())?;
    if tree != *expected_tree {
        return Err(changed());
    }
    Ok(())
}

fn validate_case_inventory(inventory: &filesystem::Tree) -> Result<(), CollectorError> {
    let expected = FIXTURES
        .into_iter()
        .chain(["dogfood"])
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for (name, entry) in inventory.iter().filter(|(name, _)| !name.contains('/')) {
        if !expected.contains(name.as_str()) {
            return Err(CollectorError::mismatch(format!(
                "unexpected top-level golden entry: {name}"
            )));
        }
        if !matches!(entry, filesystem::Entry::Directory) {
            return Err(CollectorError::mismatch(format!(
                "top-level golden entry is not a real directory: {name}"
            )));
        }
        actual.insert(name.as_str());
    }
    if let Some(missing) = expected.iter().find(|name| !actual.contains(**name)) {
        return Err(CollectorError::mismatch(format!(
            "missing top-level golden case: {missing}"
        )));
    }
    Ok(())
}

fn case_tree(name: &str, inventory: &filesystem::Tree) -> filesystem::Tree {
    let prefix = format!("{name}/");
    inventory
        .iter()
        .filter_map(|(path, entry)| {
            path.strip_prefix(&prefix)
                .map(|relative| (relative.to_owned(), entry.clone()))
        })
        .collect()
}

fn case_trees(inventory: &filesystem::Tree) -> BTreeMap<String, filesystem::Tree> {
    FIXTURES
        .into_iter()
        .chain(["dogfood"])
        .map(|name| (name.to_owned(), case_tree(name, inventory)))
        .collect()
}

fn changed() -> CollectorError {
    CollectorError::mismatch("golden oracle changed during candidate execution")
}
