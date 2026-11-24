// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use velnor_ci::cli::{ImageStageArgs, OpensslMode};
use velnor_ci::io::image_deploy::stage;

use super::elf::{Elf, build};
use crate::support::fresh_dir;

const TARGET: &str = "x86_64-unknown-linux-gnu/release";

fn args(root: &Path, glibc: &str, openssl: OpensslMode, bins: &[&str]) -> ImageStageArgs {
    ImageStageArgs {
        target_dir: root.join("target"),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        profile: "release".to_owned(),
        glibc: glibc.to_owned(),
        openssl,
        to: root.join("dist"),
        bins: bins.iter().map(|bin| (*bin).to_owned()).collect(),
    }
}

fn built(root: &Path, name: &str, elf: &Elf) {
    let directory = root.join("target").join(TARGET);
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(directory.join(name), build(elf)).unwrap_or_else(|e| unreachable!("{e}"));
}

fn refusal(args: &ImageStageArgs) -> String {
    stage(args)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

#[test]
fn a_dynamic_x86_64_binary_within_the_glibc_ceiling_is_staged_as_a_real_executable_file() {
    let root = fresh_dir("deploy", "stage-ok");
    built(&root, "svc", &Elf::dynamic_x86_64());
    let report = stage(&args(&root, "2.41", OpensslMode::None, &["svc"]));
    let report = report.unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        report.contains("svc: x86-64 dynamic, glibc ceiling 2.34 (allowed 2.41)"),
        "{report}"
    );
    let staged = root.join("dist/svc");
    assert_eq!(
        fs::read(&staged).unwrap_or_default(),
        build(&Elf::dynamic_x86_64())
    );
    let metadata = fs::symlink_metadata(&staged).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
}

#[test]
fn a_target_directory_that_is_a_link_is_followed_into_real_files() {
    let root = fresh_dir("deploy", "stage-link");
    let store = fresh_dir("deploy", "stage-link-store");
    built(&store, "svc", &Elf::dynamic_x86_64());
    symlink(store.join("target"), root.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(stage(&args(&root, "2.41", OpensslMode::None, &["svc"])).is_ok());
    let metadata =
        fs::symlink_metadata(root.join("dist/svc")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
}

#[test]
fn the_glibc_ceiling_decides_between_staging_and_refusing_and_nothing_is_staged_on_a_refusal() {
    let root = fresh_dir("deploy", "stage-glibc");
    built(&root, "good", &Elf::dynamic_x86_64());
    built(
        &root,
        "newer",
        &Elf::dynamic_x86_64().with_versions(&["GLIBC_2.17", "GLIBC_2.43"]),
    );
    assert!(stage(&args(&root, "2.43", OpensslMode::None, &["newer"])).is_ok());
    fs::remove_dir_all(root.join("dist")).unwrap_or_else(|e| unreachable!("{e}"));
    let message = refusal(&args(&root, "2.41", OpensslMode::None, &["good", "newer"]));
    assert!(message.contains("it needs GLIBC_2.43"), "{message}");
    assert!(message.contains("allowed glibc 2.41"), "{message}");
    assert!(!root.join("dist").exists(), "a refusal must stage nothing");
}

#[test]
fn a_binary_that_is_not_x86_64_dynamic_or_clean_of_build_machine_paths_is_refused() {
    let root = fresh_dir("deploy", "stage-shape");
    let cases = [
        (
            "arm",
            Elf::dynamic_x86_64().aarch64(),
            "not an x86-64 binary",
        ),
        (
            "static",
            Elf::dynamic_x86_64().without_interpreter(),
            "statically linked",
        ),
        (
            "rpath",
            Elf::dynamic_x86_64().with_library_path("/home/runner/work/target/release/deps"),
            "library search path /home/runner/work",
        ),
    ];
    for (name, elf, expected) in cases {
        built(&root, name, &elf);
        let message = refusal(&args(&root, "2.41", OpensslMode::None, &[name]));
        assert!(message.contains(expected), "{name}: {message}");
    }
    built(
        &root,
        "origin",
        &Elf::dynamic_x86_64().with_library_path("$ORIGIN/../lib"),
    );
    assert!(stage(&args(&root, "2.41", OpensslMode::None, &["origin"])).is_ok());
}

#[test]
fn a_missing_binary_a_non_elf_file_and_a_bad_name_are_refused() {
    let root = fresh_dir("deploy", "stage-missing");
    let directory = root.join("target").join(TARGET);
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(directory.join("script"), "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        refusal(&args(&root, "2.41", OpensslMode::None, &["ghost"])).contains("it was not built")
    );
    assert!(
        refusal(&args(&root, "2.41", OpensslMode::None, &["script"])).contains("not an ELF file")
    );
    assert!(
        refusal(&args(&root, "2.41", OpensslMode::None, &["../etc"])).contains("not a binary name")
    );
}

#[test]
fn a_stage_directory_that_already_holds_files_is_refused_and_an_empty_one_is_used() {
    let root = fresh_dir("deploy", "stage-dir");
    built(&root, "svc", &Elf::dynamic_x86_64());
    fs::create_dir_all(root.join("dist")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(stage(&args(&root, "2.41", OpensslMode::None, &["svc"])).is_ok());
    assert!(refusal(&args(&root, "2.41", OpensslMode::None, &["svc"])).contains("is not empty"));
    assert!(
        refusal(&args(&root, "two", OpensslMode::None, &["svc"])).contains("not a glibc version")
    );
}

#[test]
fn openssl_in_the_binary_must_match_the_declared_handling() {
    let root = fresh_dir("deploy", "stage-openssl");
    let linked = Elf::dynamic_x86_64()
        .needing("libssl.so.3")
        .needing("libcrypto.so.3");
    built(&root, "tls", &linked);
    for mode in [OpensslMode::None, OpensslMode::Vendored] {
        let message = refusal(&args(&root, "2.41", mode, &["tls"]));
        assert!(message.contains("it needs libssl.so.3"), "{message}");
        assert!(!root.join("dist").exists());
    }
    assert!(stage(&args(&root, "2.41", OpensslMode::Sysroot, &["tls"])).is_ok());
    fs::remove_dir_all(root.join("dist")).unwrap_or_else(|e| unreachable!("{e}"));
    built(
        &root,
        "old",
        &Elf::dynamic_x86_64().needing("libssl.so.1.1"),
    );
    let message = refusal(&args(&root, "2.41", OpensslMode::Sysroot, &["old"]));
    assert!(
        message.contains("provides only libssl.so.3 and libcrypto.so.3"),
        "{message}"
    );
    built(&root, "plain", &Elf::dynamic_x86_64());
    assert!(stage(&args(&root, "2.41", OpensslMode::Sysroot, &["plain"])).is_ok());
}

#[test]
fn a_binary_named_twice_is_refused_before_anything_is_staged() {
    let root = fresh_dir("deploy", "stage-twice");
    built(&root, "svc", &Elf::dynamic_x86_64());
    let message = refusal(&args(&root, "2.41", OpensslMode::None, &["svc", "svc"]));
    assert!(message.contains("--bin svc is given twice"), "{message}");
    assert!(!root.join("dist").exists());
}

#[test]
fn a_dependency_recorded_as_a_build_machine_path_is_refused() {
    let root = fresh_dir("deploy", "stage-absolute");
    built(
        &root,
        "svc",
        &Elf::dynamic_x86_64().needing("/tmp/build/libfoo.so"),
    );
    let message = refusal(&args(&root, "2.41", OpensslMode::None, &["svc"]));
    assert!(
        message.contains("it needs /tmp/build/libfoo.so, a path on the build machine"),
        "{message}"
    );
    assert!(!root.join("dist").exists());
}

#[test]
fn an_empty_search_path_component_a_foreign_loader_and_the_relr_marker_are_refused() {
    let root = fresh_dir("deploy", "stage-loader");
    let cases = [
        (
            "emptypath",
            Elf::dynamic_x86_64().with_library_path(":$ORIGIN/lib"),
            "empty library search path component",
        ),
        (
            "loader",
            Elf::dynamic_x86_64().with_loader("/tmp/nonexistent-loader"),
            "program interpreter is /tmp/nonexistent-loader",
        ),
        (
            "relr",
            Elf::dynamic_x86_64().with_versions(&["GLIBC_2.17", "GLIBC_ABI_DT_RELR"]),
            "it needs GLIBC_2.36",
        ),
    ];
    for (name, elf, expected) in cases {
        built(&root, name, &elf);
        let message = refusal(&args(&root, "2.34", OpensslMode::None, &[name]));
        assert!(message.contains(expected), "{name}: {message}");
    }
    built(
        &root,
        "relr-ok",
        &Elf::dynamic_x86_64().with_versions(&["GLIBC_ABI_DT_RELR"]),
    );
    assert!(stage(&args(&root, "2.36", OpensslMode::None, &["relr-ok"])).is_ok());
}

#[test]
fn staged_binaries_are_executable_whatever_the_umask() {
    let root = fresh_dir("deploy", "stage-umask");
    built(&root, "svc", &Elf::dynamic_x86_64());
    let previous = nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o077));
    let staged = stage(&args(&root, "2.41", OpensslMode::None, &["svc"]));
    nix::sys::stat::umask(previous);
    assert!(staged.is_ok());
    let mode = fs::metadata(root.join("dist/svc")).map(|m| m.permissions().mode() & 0o777);
    assert_eq!(mode.ok(), Some(0o755));
}
