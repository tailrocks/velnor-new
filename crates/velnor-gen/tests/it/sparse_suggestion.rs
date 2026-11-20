// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::facts::TestResult;
use crate::pipeline::view;
use velnor_gen::sparse_note;
use velnor_model::{RepoPath, RepoView};

const MIB: u64 = 1024 * 1024;
const OFF: &str = "full_run = \"off\"\n";
const ON: &str = "full_run = \"off\"\nsparse_checkout = true\n";
const ROOTS: &[&str] = &["a/Cargo.toml", "b/Cargo.toml"];

fn sizes(entries: &[(&str, u64)]) -> Result<BTreeMap<RepoPath, u64>, Box<dyn std::error::Error>> {
    entries
        .iter()
        .map(|(path, size)| Ok((RepoPath::new(*path)?, *size)))
        .collect()
}

fn with(config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    view(&ROOTS, Some(config.as_bytes()))
}

#[test]
fn generate_suggests_sparse_checkout_only_above_100_mib() -> TestResult {
    let rust = [("a/src/lib.rs", 40 * MIB), ("Cargo.lock", MIB)];
    let over = sizes(&[rust[0], rust[1], ("bench/archive.tar", 100 * MIB + 1)])?;
    let exact = sizes(&[rust[0], rust[1], ("bench/archive.tar", 100 * MIB)])?;
    let note = sparse_note(&with(OFF)?, &over)?.ok_or("a tree over 100 MiB outside Rust paths")?;
    assert!(note.contains("sparse_checkout = true"), "{note}");
    assert!(note.contains(".velnor/rust-ci.toml"), "{note}");
    assert!(note.contains("100 MiB"), "{note}");
    assert_eq!(sparse_note(&with(OFF)?, &exact)?, None);
    assert_eq!(sparse_note(&with(ON)?, &over)?, None);
    Ok(())
}

#[test]
fn files_the_lane_reads_do_not_count_toward_the_suggestion() -> TestResult {
    let read = sizes(&[
        ("a/assets/big.bin", 200 * MIB),
        ("b/Cargo.toml", 1),
        (".velnor/rust-inputs.lock", 1),
    ])?;
    assert_eq!(sparse_note(&with(OFF)?, &read)?, None);
    let declared = format!("{OFF}[[inputs]]\nglob = \"shared/**\"\npackages = [\"package-0\"]\n");
    let shared = sizes(&[("shared/data.bin", 200 * MIB)])?;
    assert_eq!(sparse_note(&with(&declared)?, &shared)?, None);
    assert!(sparse_note(&with(OFF)?, &shared)?.is_some());
    Ok(())
}

#[test]
fn a_repository_that_cannot_go_sparse_gets_no_suggestion() -> TestResult {
    let big = sizes(&[("bench/archive.tar", 200 * MIB)])?;
    let top = view(&["Cargo.toml"], Some(OFF.as_bytes()))?;
    assert_eq!(sparse_note(&top, &big)?, None);
    Ok(())
}

#[test]
fn a_one_job_repository_gets_no_suggestion_because_its_job_renders_no_sparse_list() -> TestResult {
    let big = sizes(&[("bench/archive.tar", 200 * MIB)])?;
    let one_job = view(&["a/Cargo.toml"], Some(OFF.as_bytes()))?;
    assert_eq!(sparse_note(&one_job, &big)?, None);
    Ok(())
}

#[test]
fn a_multi_job_repository_still_gets_the_suggestion() -> TestResult {
    let big = sizes(&[("bench/archive.tar", 200 * MIB)])?;
    let split = format!("{OFF}shape = \"lint-test\"\n");
    let one_root = view(&["a/Cargo.toml"], Some(split.as_bytes()))?;
    assert!(sparse_note(&one_root, &big)?.is_some());
    Ok(())
}
