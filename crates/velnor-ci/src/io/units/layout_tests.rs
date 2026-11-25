// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The build directory layout Cargo 1.100 writes, mirrored from a real `cargo build` of a
//! workspace with a dependency that has a build script (`dep`) and a binary (`fx`).

use std::fs;
use std::path::Path;

use super::workspace::Roots;
use super::{UnitFile, Workspace, unit_file, unit_files_within};

const LIB: &str = "debug/build/dep/5c64b2dee9af80ce";
const SCRIPT: &str = "debug/build/dep/3f722b21ff7c4711";
const RUN: &str = "debug/build/dep/33cec232bab789d1";
const BIN: &str = "debug/build/fx/3374f0e803714d87";

fn parsed(path: &str) -> Option<UnitFile> {
    unit_file(Path::new(path))
}

fn fingerprint(unit: &str, package: &str, hash: &str, name: &str) {
    assert_eq!(
        parsed(&format!("{unit}/fingerprint/{name}")),
        Some(UnitFile::Fingerprint {
            configuration: "debug".to_owned(),
            package: package.to_owned(),
            hash: hash.to_owned(),
            name: name.to_owned(),
        }),
        "{unit}/fingerprint/{name}"
    );
}

#[test]
fn fingerprints_of_every_unit_kind_are_read_from_the_unit_directory() {
    fingerprint(LIB, "dep", "5c64b2dee9af80ce", "lib-dep");
    fingerprint(LIB, "dep", "5c64b2dee9af80ce", "dep-lib-dep");
    fingerprint(
        SCRIPT,
        "dep",
        "3f722b21ff7c4711",
        "build-script-build-script-build",
    );
    fingerprint(
        RUN,
        "dep",
        "33cec232bab789d1",
        "run-build-script-build-script-build",
    );
    fingerprint(BIN, "fx", "3374f0e803714d87", "bin-fx");
}

#[test]
fn dep_info_carries_the_hash_of_its_unit_directory() {
    for (unit, stem, hash) in [
        (LIB, "dep-5c64b2dee9af80ce", "5c64b2dee9af80ce"),
        (BIN, "fx", "3374f0e803714d87"),
    ] {
        assert_eq!(
            parsed(&format!("{unit}/out/{stem}.d")),
            Some(UnitFile::DepInfo {
                configuration: "debug".to_owned(),
                hash: Some(hash.to_owned()),
                stem: stem.to_owned(),
            }),
            "{unit}/out/{stem}.d"
        );
    }
}

#[test]
fn a_build_script_and_its_run_are_told_apart() {
    assert_eq!(
        parsed(&format!("{SCRIPT}/out/build_script_build.d")),
        Some(UnitFile::Script {
            profile: "debug".to_owned(),
            package: "dep".to_owned(),
        })
    );
    assert_eq!(
        parsed(&format!("{RUN}/run/stdout")),
        Some(UnitFile::BuildOutput {
            configuration: "debug".to_owned(),
            package: "dep".to_owned(),
            hash: "33cec232bab789d1".to_owned(),
        })
    );
    for ignored in ["stderr", "root-output", "invoked.timestamp"] {
        assert_eq!(parsed(&format!("{RUN}/run/{ignored}")), None, "{ignored}");
    }
}

#[test]
fn a_target_triple_stays_part_of_the_configuration() {
    assert_eq!(
        parsed("x86_64-unknown-linux-gnu/debug/build/dep/5c64b2dee9af80ce/fingerprint/lib-dep"),
        Some(UnitFile::Fingerprint {
            configuration: "x86_64-unknown-linux-gnu/debug".to_owned(),
            package: "dep".to_owned(),
            hash: "5c64b2dee9af80ce".to_owned(),
            name: "lib-dep".to_owned(),
        })
    );
}

#[test]
fn files_a_build_script_generated_are_not_dep_info() {
    let root = std::env::temp_dir().join(format!("velnor-units-{}", std::process::id()));
    let unit = root.join(RUN);
    for folder in ["run", "out"] {
        fs::create_dir_all(unit.join(folder)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::write(unit.join("out/generated.d"), "not dep-info").unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(unit.join("run/stdout"), "").unwrap_or_else(|e| unreachable!("{e}"));
    let found = unit_files_within(&root, 100).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(matches!(found[0].0, UnitFile::BuildOutput { .. }));
}

#[test]
fn the_old_layout_is_still_read() {
    assert_eq!(
        parsed("debug/.fingerprint/dep-5c64b2dee9af80ce/lib-dep"),
        Some(UnitFile::Fingerprint {
            configuration: "debug".to_owned(),
            package: "dep".to_owned(),
            hash: "5c64b2dee9af80ce".to_owned(),
            name: "lib-dep".to_owned(),
        })
    );
    assert!(matches!(
        parsed("debug/deps/dep-5c64b2dee9af80ce.d"),
        Some(UnitFile::DepInfo { hash: Some(_), .. })
    ));
    assert!(matches!(
        parsed("debug/build/dep-33cec232bab789d1/output"),
        Some(UnitFile::BuildOutput { .. })
    ));
}

pub(super) fn target_with(files: &[&str]) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "velnor-units-built-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    for file in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(path, "").unwrap_or_else(|e| unreachable!("{e}"));
    }
    root
}

#[test]
fn a_built_target_directory_with_no_unit_in_a_known_layout_is_refused() {
    let root = target_with(&[
        "debug/CACHEDIR.TAG",
        "debug/units/dep/libdep.rlib",
        "debug/units/dep/libdep.rmeta",
    ]);
    let refused = unit_files_within(&root, 100);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let message = refused
        .expect_err("a built target without units must be refused")
        .to_string();
    assert!(
        message.contains("found no units in") && message.contains("unknown cargo layout?"),
        "{message}"
    );
}

#[test]
fn a_target_directory_nothing_was_built_in_is_not_refused() {
    let root = target_with(&[
        "debug/CACHEDIR.TAG",
        "debug/.rustc_info.json",
        "debug/.cargo-lock",
    ]);
    let found = unit_files_within(&root, 100);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.unwrap_or_else(|e| unreachable!("{e}")).len(), 2);
}

#[test]
fn a_built_target_directory_with_units_in_either_layout_is_not_refused() {
    for layout in [
        [
            "debug/.fingerprint/dep-5c64b2dee9af80ce/lib-dep",
            "debug/deps/dep-5c64b2dee9af80ce.d",
        ],
        [
            "debug/build/dep/5c64b2dee9af80ce/fingerprint/lib-dep",
            "debug/build/dep/5c64b2dee9af80ce/out/dep-5c64b2dee9af80ce.d",
        ],
    ] {
        let root = target_with(&[
            layout[0],
            layout[1],
            "debug/.cargo-lock",
            "debug/libdep.rlib",
        ]);
        let found = unit_files_within(&root, 100);
        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            found.unwrap_or_else(|e| unreachable!("{e}")).len(),
            2,
            "{layout:?}"
        );
    }
}

fn sharing(root: &std::path::Path, build: &std::path::Path) -> Workspace {
    Workspace {
        root: root.join("..").join(build.file_name().unwrap_or_default()),
        target_directory: root.to_owned(),
        build_directory: build.to_owned(),
        shared_build: root == build,
    }
}

#[test]
fn a_shared_target_is_scanned_strictly_only_when_every_workspace_using_it_is_plain() {
    let root = target_with(&["debug/CACHEDIR.TAG", "debug/libapp.rlib"]);
    let plain = sharing(&root, &root);
    let apart = sharing(&root, &root.join("build"));
    let alone = Roots::new(std::slice::from_ref(&plain)).unit_files(&root);
    let mixed = Roots::new(&[apart, plain]).unit_files(&root);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let message = alone
        .expect_err("a plain target that holds outputs and no unit must be refused")
        .to_string();
    assert!(message.contains("unknown cargo layout?"), "{message}");
    assert!(
        mixed.is_ok(),
        "a target shared with a separate build directory must keep the unchecked scan"
    );
}

#[test]
fn a_plain_root_that_shares_its_target_with_a_separate_build_root_reads_its_build_units_unchecked()
{
    let root = target_with(&["debug/CACHEDIR.TAG", "debug/libapp.rlib"]);
    let group = [sharing(&root, &root.join("build")), sharing(&root, &root)];
    let found = group[1].build_units(&group);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(found.is_ok(), "{found:?}");
}

#[test]
fn a_build_directory_that_names_the_target_keeps_doing_so_after_its_link_is_retargeted() {
    let root = target_with(&["a/keep", "b/keep"]);
    let link = root.join("link");
    std::os::unix::fs::symlink(root.join("a"), &link).unwrap_or_else(|e| unreachable!("{e}"));
    let printed = serde_json::json!({
        "workspace_root": root,
        "target_directory": root.join("a"),
        "build_directory": link,
    });
    let workspace: Workspace =
        serde_json::from_value(printed).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_file(&link).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(root.join("b"), &link).unwrap_or_else(|e| unreachable!("{e}"));
    let directories = workspace.directories();
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(directories, vec![root.join("a")]);
}

#[test]
fn a_build_directory_inside_a_target_reached_through_a_link_is_not_scanned_twice() {
    let root = target_with(&[
        "debug/CACHEDIR.TAG",
        "debug/libapp.rlib",
        "bd/CACHEDIR.TAG",
        "bd/debug/CACHEDIR.TAG",
        "bd/debug/libapp.rlib",
    ]);
    let alias = root.with_file_name(format!(
        "{}-alias",
        root.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::os::unix::fs::symlink(&root, &alias).unwrap_or_else(|e| unreachable!("{e}"));
    let units = |target: &Path| {
        let workspace = Workspace {
            root: root.clone(),
            target_directory: target.to_owned(),
            build_directory: root.join("bd"),
            shared_build: false,
        };
        workspace
            .build_units(std::slice::from_ref(&workspace))
            .unwrap_or_else(|e| unreachable!("{e}"))
            .len()
    };
    let direct = units(&root);
    let reached = units(&alias);
    fs::remove_file(&alias).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(reached, direct);
}
