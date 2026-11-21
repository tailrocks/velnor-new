// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::layout_tests::target_with;
use super::unit_files_within;

const LIB: &str = "debug/build/my-lib/e0a3e98b7b3fb465";
const TOOL: &str = "debug/build/my-tool/a6639d512890f3ae";
const TOOL_EXAMPLE: &str = "debug/build/my-tool/5fc45a411dfec6b2";

fn with(unit: &str, files: &[&str]) -> Vec<String> {
    files.iter().map(|file| format!("{unit}/{file}")).collect()
}

fn lib() -> Vec<String> {
    with(
        LIB,
        &[
            "fingerprint/dep-lib-my_lib",
            "fingerprint/invoked.timestamp",
            "fingerprint/lib-my_lib",
            "fingerprint/lib-my_lib.json",
            "fingerprint/output-lib-my_lib",
            "out/libmy_lib-e0a3e98b7b3fb465.rlib",
            "out/libmy_lib-e0a3e98b7b3fb465.rmeta",
            "out/my_lib-e0a3e98b7b3fb465.d",
        ],
    )
}

fn bin_only_build() -> Vec<String> {
    let mut files = lib();
    files.extend(with(
        TOOL,
        &[
            "fingerprint/bin-my-tool",
            "fingerprint/bin-my-tool.json",
            "fingerprint/dep-bin-my-tool",
            "fingerprint/invoked.timestamp",
            "fingerprint/output-bin-my-tool",
            "out/my_tool",
            "out/my_tool.d",
        ],
    ));
    files.extend(["debug/CACHEDIR.TAG", "debug/my-tool", "debug/my-tool.d"].map(str::to_owned));
    files
}

fn example_build() -> Vec<String> {
    let mut files = lib();
    files.extend(with(
        TOOL_EXAMPLE,
        &[
            "fingerprint/dep-example-tool-ex",
            "fingerprint/example-tool-ex",
            "fingerprint/example-tool-ex.json",
            "fingerprint/invoked.timestamp",
            "fingerprint/output-example-tool-ex",
            "out/tool_ex",
            "out/tool_ex.d",
        ],
    ));
    files.extend(["debug/examples/tool-ex", "debug/examples/tool-ex.d"].map(str::to_owned));
    files
}

fn scanned(files: &[String]) -> Result<usize, String> {
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    let root = target_with(&names);
    let found = unit_files_within(&root, 1000);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    found.map(|found| found.len()).map_err(|e| e.to_string())
}

#[test]
fn the_files_cargo_wrote_for_a_binary_only_package_are_all_owned() {
    scanned(&bin_only_build()).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn the_files_cargo_wrote_for_the_examples_of_a_binary_only_package_are_all_owned() {
    scanned(&example_build()).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn the_dep_info_beside_a_hyphenated_binary_is_owned_by_its_underscored_unit_only() {
    let mut files = bin_only_build();
    files.push("debug/my-other.d".to_owned());
    scanned(&files).expect_err("a name no unit produced must be refused");
}

#[test]
fn an_output_no_unit_of_its_directory_names_is_refused_wherever_it_is() {
    for orphan in [
        format!("{LIB}/out/libzzz.rlib"),
        format!("{LIB}/out/libzzz-e0a3e98b7b3fb465.rmeta"),
        format!("{LIB}/out/libmy_lib_extra-e0a3e98b7b3fb465.rlib"),
        format!("{LIB}/fingerprint/libzzz.rlib"),
        format!("{LIB}/fingerprint/libmy_lib.rlib"),
        format!("{LIB}/fingerprint/zzz.d"),
        format!("{TOOL}/run/libzzz.rlib"),
        "debug/build/ghost/e0a3e98b7b3fb465/out/libghost-e0a3e98b7b3fb465.rlib".to_owned(),
        "debug/libapp.rlib".to_owned(),
        "debug/examples/libzzz.rlib".to_owned(),
    ] {
        let mut files = bin_only_build();
        files.push(orphan.clone());
        let message = scanned(&files).expect_err(&orphan);
        assert!(
            message.contains("unknown cargo layout?"),
            "{orphan}: {message}"
        );
    }
}

#[test]
fn an_output_of_a_unit_in_its_own_directory_is_owned_whatever_its_hash_or_spelling() {
    for owned in [
        format!("{LIB}/out/libmy_lib-0123456789abcdef.rmeta"),
        format!("{LIB}/out/libmy_lib.rlib"),
    ] {
        let mut files = bin_only_build();
        files.push(owned.clone());
        scanned(&files).unwrap_or_else(|e| unreachable!("{owned}: {e}"));
    }
}
