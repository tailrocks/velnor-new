// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The build directory layout Cargo 1.100 writes, mirrored from a real `cargo build` of a
//! workspace with a dependency that has a build script (`dep`) and a binary (`fx`).

use std::fs;
use std::path::Path;

use super::{UnitFile, unit_file, unit_files_within};

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
                configuration: Some("debug".to_owned()),
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
