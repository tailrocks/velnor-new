// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use super::gc1_t29_common::{
    PREVIOUS_CC_ENV, PREVIOUS_CXX_ENV, PREVIOUS_RUSTC_ENV, PREVIOUS_RUSTDOC_ENV, binary, case_dir,
    cleanup, entry_link, rejected, rejected_by_observer, run_entry, scratch_root,
    write_fake_compiler,
};

fn check_rejects_rustc_without_compiler(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-rustc-bare")?;
    let entry = entry_link(&dir, "rustc")?;
    rejected_by_observer(&run_entry(&entry, &[], &[])?, "rustc without real compiler")?;
    Ok(())
}

fn check_rejects_rustc_recursion(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-rustc-recursion")?;
    let entry = entry_link(&dir, "rustc")?;
    let output = run_entry(&entry, &[binary().to_string_lossy().as_ref()], &[])?;
    rejected_by_observer(&output, "rustc delegating to itself")?;
    let output = run_entry(
        &entry,
        &["/usr/bin/rustc"],
        &[(PREVIOUS_RUSTC_ENV, binary().to_string_lossy().as_ref())],
    )?;
    rejected_by_observer(&output, "previous wrapper delegating to itself")?;
    Ok(())
}

fn check_rejects_empty_previous_wrapper(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-prev-empty")?;
    let log = dir.join("empty.log");
    let real = write_fake_compiler(&dir, "real-prev-empty", "real", &log, 0)?;
    let entry = entry_link(&dir, "rustc")?;
    let output = run_entry(
        &entry,
        &[real.to_string_lossy().as_ref()],
        &[(PREVIOUS_RUSTC_ENV, "")],
    )?;
    rejected_by_observer(&output, "empty previous wrapper")?;
    Ok(())
}

fn check_rejects_rustdoc_without_context(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-rustdoc")?;
    let entry = entry_link(&dir, "rustdoc")?;
    rejected_by_observer(
        &run_entry(&entry, &["--crate-name", "foo"], &[])?,
        "rustdoc without validated real compiler",
    )?;
    let output = run_entry(
        &entry,
        &["--crate-name", "foo"],
        &[(PREVIOUS_RUSTDOC_ENV, "relative/rustdoc")],
    )?;
    rejected_by_observer(&output, "relative rustdoc executable")?;
    let output = run_entry(
        &entry,
        &["--crate-name", "foo"],
        &[(PREVIOUS_RUSTDOC_ENV, "")],
    )?;
    rejected_by_observer(&output, "empty rustdoc executable")?;
    Ok(())
}

fn check_rejects_native_without_context(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-native")?;
    let cc_entry = entry_link(&dir, "cc")?;
    rejected_by_observer(
        &run_entry(&cc_entry, &["-c", "src/native.c"], &[])?,
        "cc without validated real compiler",
    )?;
    let output = run_entry(
        &cc_entry,
        &["-c", "src/native.c"],
        &[(PREVIOUS_CC_ENV, "relative/cc")],
    )?;
    rejected_by_observer(&output, "relative cc executable")?;
    let cxx_entry = entry_link(&dir, "cxx")?;
    rejected_by_observer(
        &run_entry(&cxx_entry, &["-c", "src/native.cpp"], &[])?,
        "cxx without validated real compiler",
    )?;
    let output = run_entry(
        &cxx_entry,
        &["-c", "src/native.cpp"],
        &[(PREVIOUS_CXX_ENV, "relative/c++")],
    )?;
    rejected_by_observer(&output, "relative cxx executable")?;
    Ok(())
}

fn check_rejects_unapproved_entry(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-entry")?;
    let entry = entry_link(&dir, "gcc")?;
    rejected(
        &run_entry(&entry, &["-c", "src/native.c"], &[])?,
        "unapproved compiler entry",
    )?;
    Ok(())
}

fn check_rejects_missing_executable(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "reject-missing")?;
    let entry = entry_link(&dir, "rustc")?;
    let missing = dir.join("does-not-exist-rustc");
    rejected_by_observer(
        &run_entry(&entry, &[missing.to_string_lossy().as_ref()], &[])?,
        "missing real compiler",
    )?;
    Ok(())
}

#[test]
fn gc1_t29_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let root = scratch_root()?;
    let outcome = (|| {
        check_rejects_rustc_without_compiler(&root)?;
        check_rejects_rustc_recursion(&root)?;
        check_rejects_empty_previous_wrapper(&root)?;
        check_rejects_rustdoc_without_context(&root)?;
        check_rejects_native_without_context(&root)?;
        check_rejects_unapproved_entry(&root)?;
        check_rejects_missing_executable(&root)
    })();
    cleanup(&root);
    outcome
}
