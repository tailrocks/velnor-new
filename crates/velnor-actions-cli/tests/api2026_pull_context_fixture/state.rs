// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
};

pub(super) type Tree = BTreeMap<PathBuf, Option<Vec<u8>>>;

#[cfg(target_arch = "aarch64")]
pub(super) fn before(
    scenario: super::Scenario,
    root: &Path,
) -> Option<Result<Tree, Box<dyn Error>>> {
    matches!(scenario, super::Scenario::UnsupportedHost).then(|| snapshot(root))
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) fn before(_: super::Scenario, _: &Path) -> Option<Result<Tree, Box<dyn Error>>> {
    None
}

pub(super) fn verify(
    root: &Path,
    before: Option<Tree>,
    requests: &str,
) -> Result<(), Box<dyn Error>> {
    if let Some(before) = before {
        assert!(
            requests.is_empty(),
            "unsupported host contacted the provider"
        );
        let after = snapshot(root)?;
        let changed = before
            .keys()
            .chain(after.keys())
            .filter(|path| before.get(*path) != after.get(*path))
            .collect::<BTreeSet<_>>();
        assert!(
            changed.is_empty(),
            "unsupported host changed private paths: {changed:?}"
        );
    }
    Ok(())
}

fn snapshot(root: &Path) -> Result<Tree, Box<dyn Error>> {
    let mut tree = Tree::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            let key = path.strip_prefix(root)?.to_owned();
            if kind.is_dir() {
                tree.insert(key, None);
                pending.push(path);
            } else if kind.is_file() {
                tree.insert(key, Some(fs::read(path)?));
            } else {
                return Err("unexpected fixture entry".into());
            }
        }
    }
    Ok(tree)
}
