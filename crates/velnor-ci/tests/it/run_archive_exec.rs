// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::scratch;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use velnor_test_support::real_cargo;

const MANIFEST: &str =
    "[package]\nname = \"fx\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n";
const BUILD: &str = "fn main() {\n    use std::os::unix::fs::PermissionsExt;\n    let helper = std::path::PathBuf::from(std::env::var(\"OUT_DIR\").unwrap()).join(\"helper\");\n    std::fs::write(&helper, \"#!/bin/sh\\nexit 0\\n\").unwrap();\n    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();\n}\n";

fn write_fixture(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(dir.join("tests")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("Cargo.toml"), MANIFEST).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("build.rs"), BUILD).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("src/lib.rs"), "").unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join("tests/t.rs"), "#[test]\nfn t() {}\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn nextest(dir: &Path, target: &Path, args: &[&str]) -> std::process::Output {
    Command::new(real_cargo())
        .arg("nextest")
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn find_named(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|file| file == name) {
            return Some(path);
        }
    }
    None
}

#[test]
fn nextest_archive_extract_keeps_the_exec_bit_of_build_script_output() {
    let root = scratch("archive-exec-bit");
    let source = root.join("source");
    write_fixture(&source);
    let archive = root.join("archive.tar.zst");
    let built = nextest(
        &source,
        &root.join("target"),
        &["archive", "--archive-file", &archive.display().to_string()],
    );
    assert!(built.status.success(), "{built:?}");

    let extracted = root.join("extracted");
    write_fixture(&extracted);
    let ran = nextest(
        &extracted,
        &root.join("unused-target"),
        &[
            "run",
            "--archive-file",
            &archive.display().to_string(),
            "--extract-to",
            ".",
            "--extract-overwrite",
            "--workspace-remap",
            ".",
        ],
    );
    assert!(ran.status.success(), "{ran:?}");

    let helper = find_named(&extracted, "helper").unwrap_or_else(|| panic!("{ran:?}"));
    let mode = fs::metadata(&helper)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "{}: mode {mode:o}", helper.display());
}
