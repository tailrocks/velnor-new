// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use super::gc1_t29_common::{
    PREVIOUS_CC_ENV, PREVIOUS_CXX_ENV, PREVIOUS_RUSTC_ENV, PREVIOUS_RUSTDOC_ENV, binary, case_dir,
    cleanup, entry_link, fail, read_log, run_entry, scratch_root, succeeded, write_fake_compiler,
    write_forwarding_wrapper,
};

fn check_rustc_delegates(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "delegates")?;
    let log = dir.join("rustc.log");
    let real = write_fake_compiler(&dir, "real-rustc", "real", &log, 0)?;
    let entry = entry_link(&dir, "rustc")?;
    let output = run_entry(
        &entry,
        &[
            real.to_string_lossy().as_ref(),
            "--crate-name",
            "foo",
            "-DFOO=bar baz",
            "src/lib.rs",
        ],
        &[],
    )?;
    succeeded(&output, "rustc delegation")?;
    let logged = read_log(&log)?;
    if !logged.contains("real:--crate-name foo -DFOO=bar baz src/lib.rs") {
        return Err(fail(&format!("rustc argv not preserved: {logged}")));
    }
    Ok(())
}

fn check_exit_status_preserved(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "status")?;
    let log = dir.join("status.log");
    let real = write_fake_compiler(&dir, "real-status", "real", &log, 42)?;
    let entry = entry_link(&dir, "rustc")?;
    let output = run_entry(&entry, &[real.to_string_lossy().as_ref()], &[])?;
    if output.status.code() != Some(42) {
        return Err(fail("observer must preserve the real compiler exit status"));
    }
    Ok(())
}

fn check_clippy_preserved(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "clippy")?;
    let log = dir.join("clippy.log");
    let clippy = write_fake_compiler(&dir, "fake-clippy", "clippy", &log, 0)?;
    let entry = entry_link(&dir, "rustc")?;
    let output = run_entry(
        &entry,
        &[
            clippy.to_string_lossy().as_ref(),
            "--edition=2021",
            "src/lib.rs",
        ],
        &[],
    )?;
    succeeded(&output, "clippy-driver as real compiler")?;
    if !read_log(&log)?.contains("clippy:--edition=2021 src/lib.rs") {
        return Err(fail("clippy-driver argv not preserved"));
    }
    let alias = entry_link(&dir, "clippy-driver")?;
    let output = run_entry(&alias, &[clippy.to_string_lossy().as_ref()], &[])?;
    succeeded(&output, "clippy-driver entry")?;
    Ok(())
}

fn check_previous_wrapper_nesting(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "nesting")?;
    let real_log = dir.join("nested-real.log");
    let prev_log = dir.join("nested-prev.log");
    let real = write_fake_compiler(&dir, "nested-real", "real", &real_log, 0)?;
    let previous = write_forwarding_wrapper(&dir, "nested-prev", "prev", &prev_log)?;
    let entry = entry_link(&dir, "rustc")?;
    let output = run_entry(
        &entry,
        &[real.to_string_lossy().as_ref(), "--crate-name", "nested"],
        &[(PREVIOUS_RUSTC_ENV, previous.to_string_lossy().as_ref())],
    )?;
    succeeded(&output, "nested previous wrapper")?;
    let seen = read_log(&prev_log)?;
    if !seen.contains(&format!(
        "prev:{} --crate-name nested",
        real.to_string_lossy()
    )) {
        return Err(fail(&format!("previous wrapper chain broken: {seen}")));
    }
    if !read_log(&real_log)?.contains("real:--crate-name nested") {
        return Err(fail("real compiler behind previous wrapper never ran"));
    }
    Ok(())
}

fn check_rustdoc_entry(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "rustdoc")?;
    let log = dir.join("rustdoc.log");
    let real = write_fake_compiler(&dir, "real-rustdoc", "doc", &log, 0)?;
    let entry = entry_link(&dir, "rustdoc")?;
    let output = run_entry(
        &entry,
        &["--crate-name", "foo", "--output", "target/doc"],
        &[(PREVIOUS_RUSTDOC_ENV, real.to_string_lossy().as_ref())],
    )?;
    succeeded(&output, "rustdoc delegation")?;
    if !read_log(&log)?.contains("doc:--crate-name foo --output target/doc") {
        return Err(fail("rustdoc argv not preserved"));
    }
    Ok(())
}

fn check_native_entries(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "native")?;
    let cc_log = dir.join("cc.log");
    let cxx_log = dir.join("cxx.log");
    let cc = write_fake_compiler(&dir, "real-cc", "cc", &cc_log, 0)?;
    let cxx = write_fake_compiler(&dir, "real-cxx", "cxx", &cxx_log, 0)?;
    let cc_entry = entry_link(&dir, "cc")?;
    let output = run_entry(
        &cc_entry,
        &["-O2", "-DFOO=bar baz", "-c", "src/native.c"],
        &[(PREVIOUS_CC_ENV, cc.to_string_lossy().as_ref())],
    )?;
    succeeded(&output, "cc delegation")?;
    if !read_log(&cc_log)?.contains("cc:-O2 -DFOO=bar baz -c src/native.c") {
        return Err(fail("cc flags not preserved"));
    }
    let cxx_entry = entry_link(&dir, "cxx")?;
    let output = run_entry(
        &cxx_entry,
        &["-O2", "-c", "src/native.cpp"],
        &[(PREVIOUS_CXX_ENV, cxx.to_string_lossy().as_ref())],
    )?;
    succeeded(&output, "cxx delegation")?;
    if !read_log(&cxx_log)?.contains("cxx:-O2 -c src/native.cpp") {
        return Err(fail("cxx flags not preserved"));
    }
    Ok(())
}

fn check_generated_layout_invokes_observer(root: &Path) -> Result<(), Box<dyn Error>> {
    let dir = case_dir(root, "layout")?;
    let wrappers = dir.join("velnor").join("wrappers");
    fs::create_dir_all(&wrappers).map_err(|error| fail(&format!("create wrapper dir: {error}")))?;
    for entry in ["rustc", "rustdoc", "cc", "cxx"] {
        let link = wrappers.join(entry);
        symlink(binary(), &link).map_err(|error| fail(&format!("stage wrapper: {error}")))?;
        let target =
            fs::read_link(&link).map_err(|error| fail(&format!("read wrapper link: {error}")))?;
        if target != binary() {
            return Err(fail("wrapper entry must reuse the existing binary"));
        }
    }
    let log = dir.join("layout.log");
    let real_rustc = write_fake_compiler(&dir, "layout-rustc", "real", &log, 0)?;
    let wrapper = wrappers.join("rustc");
    let output = run_entry(
        &wrapper,
        &[real_rustc.to_string_lossy().as_ref(), "--version"],
        &[],
    )?;
    succeeded(&output, "generated RUSTC_WRAPPER layout")?;
    if !read_log(&log)?.contains("real:--version") {
        return Err(fail("generated layout did not reach the observer"));
    }
    let real_cc = write_fake_compiler(&dir, "layout-cc", "cc", &log, 0)?;
    let cc_wrapper = wrappers.join("cc");
    let output = run_entry(
        &cc_wrapper,
        &["--version"],
        &[(PREVIOUS_CC_ENV, real_cc.to_string_lossy().as_ref())],
    )?;
    succeeded(&output, "generated CC layout")?;
    if !read_log(&log)?.contains("cc:--version") {
        return Err(fail("generated CC layout did not reach the observer"));
    }
    Ok(())
}

#[test]
fn gc1_t29_preserves_contract() -> Result<(), Box<dyn Error>> {
    let root = scratch_root()?;
    let outcome = (|| {
        check_rustc_delegates(&root)?;
        check_exit_status_preserved(&root)?;
        check_clippy_preserved(&root)?;
        check_previous_wrapper_nesting(&root)?;
        check_rustdoc_entry(&root)?;
        check_native_entries(&root)?;
        check_generated_layout_invokes_observer(&root)
    })();
    cleanup(&root);
    outcome
}
