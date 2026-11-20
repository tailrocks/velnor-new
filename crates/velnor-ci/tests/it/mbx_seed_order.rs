// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::mbx_seed_copy::planned;

use crate::io::Repo;

fn put(root: &Path, path: &str, age_secs: u64) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap_or(&file)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&file, path).unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|handle| handle.set_modified(UNIX_EPOCH + Duration::from_secs(age_secs)))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_seed_cut_at_the_budget_keeps_the_newest_entries_and_their_index() {
    let repo = Repo::empty("mbx-seed-order");
    let seed = repo.root().join("seed");
    put(&seed, "cas/aa/old-blob", 1_000);
    put(&seed, "action-results/bb/old-result", 1_100);
    put(&seed, "cas/cc/new-blob", 4_000);
    put(&seed, "action-results/dd/new-result", 4_000);
    put(&seed, "task-manifests/m/new-manifest", 4_000);
    put(&seed, "executable-identities/new-id", 4_000);
    let order: Vec<String> = planned(&seed)
        .iter()
        .map(|path| {
            path.strip_prefix(&seed)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect();
    assert_eq!(
        order,
        [
            "executable-identities/new-id",
            "task-manifests/m/new-manifest",
            "action-results/dd/new-result",
            "cas/cc/new-blob",
            "action-results/bb/old-result",
            "cas/aa/old-blob",
        ]
    );
}
