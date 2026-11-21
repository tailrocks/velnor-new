// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::super::{command, velnor_ci};
use super::entries::{VARIABLE, audit_within, declared_tree};

#[test]
fn a_non_unicode_entry_limit_is_refused_and_names_the_variable() {
    let repo = declared_tree(5);
    let refused = velnor_ci()
        .env(VARIABLE, OsStr::from_bytes(b"\xff"))
        .current_dir(repo.root())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(VARIABLE),
        "{refused:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_tree_above_100000_entries_passes_under_the_default_limit() {
    let repo = declared_tree(100_001);
    let within = audit_within(&repo, None);
    assert!(within.status.success(), "{within:?}");
}

#[test]
fn every_audit_launcher_clears_an_inherited_entry_limit() {
    for launcher in [command("true"), velnor_ci()] {
        assert!(
            launcher
                .get_envs()
                .any(|(name, value)| name == VARIABLE && value.is_none()),
            "{launcher:?}"
        );
    }
    let mut direct = Vec::new();
    direct_launch_sites(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/audit"),
        &mut direct,
    );
    assert_eq!(
        direct,
        Vec::<String>::new(),
        "every audit launcher must come from the helpers in audit/mod.rs"
    );
}

const DIRECT_SPAWN: &str = concat!("Command", "::new(");
const BINARY: &str = concat!("CARGO_BIN_EXE_", "velnor-ci");
const SHELL: &str = concat!("\"", "sh", "\"");

fn direct_launch_sites(dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        if path.is_dir() {
            direct_launch_sites(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") && !path.ends_with("audit/mod.rs")
        {
            let text = fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{e}"));
            for (index, line) in text.lines().enumerate() {
                let spawns_audit = line.contains(BINARY) || line.contains(SHELL);
                if line.contains(DIRECT_SPAWN) && spawns_audit {
                    found.push(format!("{}:{}", path.display(), index + 1));
                }
            }
        }
    }
}
