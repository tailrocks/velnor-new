// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use velnor_gen::io::Checkout;
use velnor_model::RepoView;
use velnor_rust::{detect, sha256_hex};

use super::*;
use crate::io::audit::inventory::{Evidence, disturb, receipt, receipt_path};
use crate::io::audit::setup::begin;

struct Declared {
    base: PathBuf,
    root: PathBuf,
    output: PathBuf,
}

impl Drop for Declared {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.base));
    }
}

fn declared(name: &str, build: &dyn Fn(&Path)) -> Declared {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!(
            "velnor-freshness-race-{name}-{}",
            std::process::id()
        ));
    drop(fs::remove_dir_all(&base));
    let root = base.join("declared");
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    build(&root);
    let output = base.join("output");
    fs::write(&output, "cargo:rerun-if-changed=declared\n").unwrap_or_else(|e| unreachable!("{e}"));
    let targets = BTreeSet::new();
    let scanned = inventory::scan(&root, &targets, &mut Budget::new(usize::MAX))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let evidence = Evidence {
        entries: inventory::view(&scanned.entries, &root, &targets)
            .unwrap_or_else(|e| unreachable!("{e}")),
        declared: BTreeMap::new(),
        targets,
    };
    let bytes = serde_json::to_vec(&evidence).unwrap_or_else(|e| unreachable!("{e}"));
    let saved = base.join("inventory.json");
    fs::write(&saved, &bytes).unwrap_or_else(|e| unreachable!("{e}"));
    let record =
        receipt(&output, saved, sha256_hex(&bytes)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        receipt_path(&output),
        serde_json::to_vec(&record).unwrap_or_else(|e| unreachable!("{e}")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    Declared { base, root, output }
}

fn stale(declared: &Declared) -> Result<bool, CiError> {
    let built = SystemTime::now() + Duration::from_hours(1);
    let checkout = Checkout {
        root: declared.base.clone(),
        view: RepoView::default(),
    };
    let facts = detect(&checkout.view).unwrap_or_else(|e| unreachable!("{e}"));
    let mut audit =
        begin(&checkout, &facts, &[], &[], usize::MAX).unwrap_or_else(|e| unreachable!("{e}"));
    audit
        .changed_inputs(
            &declared.base,
            &["declared".to_owned()],
            built,
            &declared.output,
            false,
            usize::MAX,
        )
        .map(|changed| !changed.is_empty())
}

fn refused(result: &Result<bool, CiError>) -> bool {
    match result {
        Ok(stale) => *stale,
        Err(error) => error
            .to_string()
            .contains("changed while the audit scanned it"),
    }
}

fn nested(root: &Path) {
    fs::create_dir_all(root.join("a/b")).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn an_undisturbed_declared_directory_that_the_build_inventoried_is_fresh() {
    let declared = declared("control", &nested);
    assert!(matches!(stale(&declared), Ok(false)));
}

#[test]
fn a_nested_ancestor_replaced_during_the_audit_scan_makes_the_input_stale() {
    let declared = declared("ancestor", &nested);
    let _race = disturb(
        |directory| {
            if directory.ends_with("a/b") {
                let a = directory.parent().unwrap_or_else(|| unreachable!());
                fs::remove_dir_all(a).unwrap_or_else(|e| unreachable!("{e}"));
                fs::write(a, b"").unwrap_or_else(|e| unreachable!("{e}"));
            }
            Ok(())
        },
        |_| {},
    );
    assert!(matches!(stale(&declared), Ok(true)));
}

#[test]
fn an_alias_to_a_directory_removed_during_the_audit_scan_makes_the_input_stale() {
    let declared = declared("alias", &|root| {
        fs::create_dir(root.join("a")).unwrap_or_else(|e| unreachable!("{e}"));
        symlink("a", root.join("z")).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let _race = disturb(
        |directory| {
            if directory.ends_with("a") {
                fs::remove_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
            }
            Ok(())
        },
        |_| {},
    );
    let result = stale(&declared);
    assert!(refused(&result), "{result:?}");
}

#[test]
fn a_directory_renamed_and_replaced_by_a_link_during_the_audit_scan_makes_the_input_stale() {
    let declared = declared("renamed", &|root| {
        fs::create_dir(root.join("d")).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let _race = disturb(
        |directory| {
            if directory.ends_with("d") {
                let moved = directory.with_file_name("moved");
                fs::rename(directory, &moved).unwrap_or_else(|e| unreachable!("{e}"));
                symlink(&moved, directory).unwrap_or_else(|e| unreachable!("{e}"));
            }
            Ok(())
        },
        |_| {},
    );
    assert!(matches!(stale(&declared), Ok(true)));
}

#[test]
fn an_entry_removed_while_the_audit_lists_the_directory_is_stale_not_an_error() {
    let declared = declared("vanished", &|root| {
        for index in 0..8 {
            fs::write(root.join(format!("entry-{index}")), b"")
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    let root = declared.root.clone();
    let _race = disturb(
        |_| Ok(()),
        move |name| {
            drop(fs::remove_file(root.join(name)));
        },
    );
    assert!(matches!(stale(&declared), Ok(true)));
}

#[test]
fn a_directory_removed_before_the_audit_opens_it_is_stale_not_an_error() {
    let declared = declared("removed", &|root| {
        fs::create_dir(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(root.join("kept.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    });
    let _race = disturb(
        |directory| {
            if directory.ends_with("sub") {
                fs::remove_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
            }
            Ok(())
        },
        |_| {},
    );
    assert!(matches!(stale(&declared), Ok(true)));
}

#[test]
fn a_file_created_in_a_listed_directory_during_the_audit_scan_makes_the_input_stale() {
    let declared = declared("late", &|root| {
        fs::create_dir(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let root = declared.root.clone();
    let _race = disturb(
        move |directory| {
            if directory.ends_with("sub") {
                fs::write(root.join("late"), b"").unwrap_or_else(|e| unreachable!("{e}"));
            }
            Ok(())
        },
        |_| {},
    );
    assert!(matches!(stale(&declared), Ok(true)));
}
