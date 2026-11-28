// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_model::{ChangeSet, RepoPath, RustSelection};
use velnor_rust::{LOCK_PATH, detect, select};

fn planned_at(pin: &str, file: &str) -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut view = velnor_gen::io::read_checkout(&root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .view;
    let lock = String::from_utf8_lossy(&view.files[&LOCK_PATH]).into_owned();
    let current = lock
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| unreachable!("{lock}"))
        .to_owned();
    view.files
        .insert(LOCK_PATH, lock.replacen(&current, pin, 1).into_bytes());
    let facts = detect(&view).unwrap_or_else(|e| unreachable!("{e}"));
    let changes = ChangeSet {
        paths: BTreeSet::from([RepoPath::new(file).unwrap_or_else(|e| unreachable!("{e}"))]),
        base_files: BTreeMap::new(),
        head_files: BTreeMap::new(),
    };
    match select(&facts, &changes).into_values().next() {
        Some(RustSelection::Some(map)) => map
            .into_iter()
            .map(|(name, reason)| (name.to_string(), reason.to_string()))
            .collect(),
        Some(RustSelection::All(_)) => vec![("*".to_owned(), String::new())],
        _ => Vec::new(),
    }
}

#[test]
fn a_docs_only_change_plans_by_the_helper_the_pin_names() {
    let before = planned_at("0.1.116", "docs/goals.md");
    assert!(
        before
            .iter()
            .any(|(name, reason)| name == "velnor-runner-host" && reason == "unresolved-include"),
        "{before:?}"
    );
    assert!(
        before.iter().any(|(name, _)| name == "velnor-runner-cli"),
        "{before:?}"
    );
    assert_eq!(planned_at("0.1.117", "docs/goals.md"), []);
}
