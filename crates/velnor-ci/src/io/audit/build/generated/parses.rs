// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use velnor_rust::sha256_hex;

use super::super::super::inventory::{self, Evidence};
use super::super::super::timing;
use super::proven;
use crate::io::units::Workspace;

const UNITS: usize = 6;

struct Fixture {
    root: PathBuf,
    target: PathBuf,
    saved: PathBuf,
    workspaces: Vec<Workspace>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|e| unreachable!("{e}"))
            .join(format!("velnor-ci-parses-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let target = root.join("target");
        let store = target.join(inventory::STORE);
        fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
        let workspaces = vec![Workspace {
            workspace_root: root.clone(),
            target_directory: target.clone(),
        }];
        let mut targets = inventory::exclusions(&workspaces).unwrap_or_else(|e| unreachable!("{e}"));
        targets.insert(root.join("generated"));
        let bytes = serde_json::to_vec(&Evidence {
            entries: BTreeMap::new(),
            declared: BTreeMap::new(),
            targets,
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
        let digest = sha256_hex(&bytes);
        let saved = store.join(format!("{digest}.json"));
        fs::write(&saved, bytes).unwrap_or_else(|e| unreachable!("{e}"));
        let fixture = Self {
            root,
            target,
            saved,
            workspaces,
        };
        for unit in 0..UNITS {
            fixture.output(unit, &digest);
        }
        fixture
    }

    fn output(&self, unit: usize, digest: &str) -> PathBuf {
        let output = self
            .target
            .join(format!("debug/build/lib-f00000000000000{unit}/output"));
        fs::create_dir_all(output.parent().unwrap_or_else(|| unreachable!()))
            .and_then(|()| fs::write(&output, format!("cargo:rerun-if-changed=src{unit}\n")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let receipt = inventory::receipt(&output, self.saved.clone(), digest.to_owned())
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(
            inventory::receipt_path(&output),
            serde_json::to_vec(&receipt).unwrap_or_else(|e| unreachable!("{e}")),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        output
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn units_sharing_one_inventory_parse_it_once() {
    let fixture = Fixture::new("shared");
    let before = timing::parses();
    let found = proven(&fixture.workspaces).unwrap_or_else(|e| unreachable!("{e}"));
    let mut expected = inventory::exclusions(&fixture.workspaces).unwrap_or_else(|e| unreachable!("{e}"));
    expected.insert(fixture.root.join("generated"));
    assert_eq!(found, expected);
    assert_eq!(timing::parses() - before, 1);
}

#[test]
fn a_changed_inventory_is_never_served_from_the_cache() {
    let fixture = Fixture::new("changed");
    let output = fixture
        .target
        .join("debug/build/lib-f000000000000000/output");
    inventory::load(&output).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&fixture.saved, "{}").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(inventory::load(&output).is_err());
}
