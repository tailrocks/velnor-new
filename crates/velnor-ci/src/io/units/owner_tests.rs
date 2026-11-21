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

const OLD_UNIT: &str = "debug/build/bs-0123456789abcdef";
const GHOST: &str = "debug/build/ghost/e0a3e98b7b3fb465";

fn refused_beside_a_build(extra: &[&str]) -> String {
    let mut files = bin_only_build();
    files.extend(extra.iter().map(|file| (*file).to_owned()));
    scanned(&files).expect_err(&extra.join(" "))
}

#[test]
fn an_out_folder_whose_unit_ran_no_build_script_owns_no_output() {
    for orphan in [
        &["debug/build/zzz-0123456789abcdef/out/x.rlib"][..],
        &["debug/build/zzz-0123456789abcdef/out/x.d"],
        &[&format!("{OLD_UNIT}/out/y.rlib")],
        &[&format!("{GHOST}/out/libghost-e0a3e98b7b3fb465.rlib")],
        &[
            &format!("{GHOST}/out/libghost-e0a3e98b7b3fb465.rlib"),
            &format!("{GHOST}/run/note"),
        ],
    ] {
        let message = refused_beside_a_build(orphan);
        assert!(
            message.contains("unknown cargo layout?"),
            "{orphan:?}: {message}"
        );
    }
}

#[test]
fn an_out_folder_beside_the_output_of_its_build_script_owns_what_the_script_generated() {
    for generated in [
        &[
            &format!("{OLD_UNIT}/output"),
            &format!("{OLD_UNIT}/out/y.rlib"),
            &format!("{OLD_UNIT}/out/y.d"),
        ][..],
        &[
            &format!("{GHOST}/run/stdout"),
            &format!("{GHOST}/out/y.rlib"),
            &format!("{GHOST}/out/y.d"),
        ],
    ] {
        let mut files = bin_only_build();
        files.extend(generated.iter().map(|file| (*file).to_owned()));
        scanned(&files).unwrap_or_else(|e| unreachable!("{generated:?}: {e}"));
    }
}

#[test]
fn an_invoked_timestamp_is_bookkeeping_only_where_cargo_writes_it() {
    for misplaced in [
        "debug/invoked.timestamp".to_owned(),
        "debug/deps/invoked.timestamp".to_owned(),
        "debug/examples/invoked.timestamp".to_owned(),
        "debug/build/invoked.timestamp".to_owned(),
        "debug/build/zzz/invoked.timestamp".to_owned(),
        "debug/build/ghost/invoked.timestamp".to_owned(),
        "debug/build/ghost/e0a3e98b7b3fb465/invoked.timestamp".to_owned(),
        "debug/build/ghost/e0a3e98b7b3fb465/run/deeper/invoked.timestamp".to_owned(),
        "debug/build/ghost/short/run/invoked.timestamp".to_owned(),
        "debug/build/zzz-short/invoked.timestamp".to_owned(),
        format!("{LIB}/invoked.timestamp"),
        format!("{LIB}/out/invoked.timestamp"),
    ] {
        let message = refused_beside_a_build(&[&misplaced]);
        assert!(
            message.contains("unknown cargo layout?"),
            "{misplaced}: {message}"
        );
    }
}

#[test]
fn an_invoked_timestamp_where_cargo_writes_it_is_owned() {
    for placed in [
        &[
            &format!("{GHOST}/run/stdout"),
            &format!("{GHOST}/run/invoked.timestamp"),
        ][..],
        &[&format!("{LIB}/fingerprint/invoked.timestamp")],
        &[&format!("{OLD_UNIT}/invoked.timestamp")],
        &[&"debug/.fingerprint/foo-0123456789abcdef/invoked.timestamp".to_owned()],
    ] {
        let mut files = bin_only_build();
        files.extend(placed.iter().map(|file| (*file).to_owned()));
        scanned(&files).unwrap_or_else(|e| unreachable!("{placed:?}: {e}"));
    }
}

#[test]
fn an_artifact_in_the_fingerprint_folder_of_an_old_layout_unit_is_an_output_nothing_owns() {
    for orphan in [
        "debug/.fingerprint/foo-0123456789abcdef/libzzz.rlib",
        "debug/.fingerprint/foo-0123456789abcdef/zzz.d",
    ] {
        let message = refused_beside_a_build(&[orphan]);
        assert!(
            message.contains("unknown cargo layout?"),
            "{orphan}: {message}"
        );
    }
}

#[test]
fn a_hyphenated_dep_info_stem_owns_the_underscored_library_beside_it() {
    scanned(&[
        "debug/deps/my-lib-0123456789abcdef.d".to_owned(),
        "debug/deps/libmy_lib-0123456789abcdef.rlib".to_owned(),
    ])
    .unwrap_or_else(|e| unreachable!("{e}"));
}

const FAILED_NEW: &str = "debug/build/gen/0123456789abcdef";
const FAILED_OLD: &str = "debug/build/gen-0123456789abcdef";

fn written(unit: &str) -> Vec<String> {
    [
        "out/gen.rlib",
        "out/gen.d",
        "out/libgen-0123456789abcdef.rmeta",
    ]
    .iter()
    .map(|file| format!("{unit}/{file}"))
    .collect()
}

#[test]
fn an_out_folder_of_a_build_script_that_started_and_failed_owns_what_it_wrote() {
    for (unit, started) in [
        (FAILED_NEW, "run/invoked.timestamp"),
        (FAILED_OLD, "invoked.timestamp"),
    ] {
        let mut files = bin_only_build();
        files.extend(written(unit));
        files.push(format!("{unit}/{started}"));
        scanned(&files).unwrap_or_else(|e| unreachable!("{unit}: {e}"));
    }
}

#[test]
fn an_out_folder_is_generated_only_inside_a_unit_directory_of_a_build_folder() {
    for (unit, markers) in [
        ("debug/build/ghost", &["output", "invoked.timestamp"][..]),
        (
            "debug/build/ghost",
            &["run/stdout", "run/invoked.timestamp"],
        ),
        (
            "debug/build/ghost/short",
            &["run/stdout", "run/invoked.timestamp"],
        ),
        ("debug/build/ghost-short", &["output", "invoked.timestamp"]),
        (
            "debug/zzz/ghost/0123456789abcdef",
            &["run/stdout", "run/invoked.timestamp"],
        ),
        (
            "debug/zzz/ghost-0123456789abcdef",
            &["output", "invoked.timestamp"],
        ),
        ("debug/build/ghost/0123456789abcdef/deeper", &["run/stdout"]),
    ] {
        for marker in markers {
            let mut files = written(unit);
            files.push(format!("{unit}/{marker}"));
            let message =
                refused_beside_a_build(&files.iter().map(String::as_str).collect::<Vec<_>>());
            assert!(
                message.contains("unknown cargo layout?"),
                "{unit} {marker}: {message}"
            );
        }
    }
}

#[test]
fn an_invoked_timestamp_outside_a_build_folder_is_not_bookkeeping() {
    for misplaced in [
        "debug/zzz/ghost/0123456789abcdef/run/invoked.timestamp",
        "debug/zzz/foo-0123456789abcdef/invoked.timestamp",
    ] {
        let message = refused_beside_a_build(&[misplaced]);
        assert!(
            message.contains("unknown cargo layout?"),
            "{misplaced}: {message}"
        );
    }
}

fn renamed(files: Vec<String>) -> Vec<String> {
    files
        .into_iter()
        .filter(|file| !file.starts_with("debug/CACHEDIR"))
        .map(|file| file.replacen("debug/", "examples/", 1))
        .collect()
}

#[test]
fn a_profile_named_examples_owns_its_final_artifacts() {
    let mut files = renamed(example_build());
    files.extend(["examples/libmy_lib.rlib", "examples/libmy_lib.d"].map(str::to_owned));
    scanned(&files).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_profile_named_examples_still_refuses_an_output_no_unit_owns() {
    for orphan in [
        "examples/libzzz.rlib",
        "examples/zzz.d",
        "examples/examples/libzzz.rlib",
    ] {
        let mut files = renamed(example_build());
        files.push(orphan.to_owned());
        let message = scanned(&files).expect_err(orphan);
        assert!(
            message.contains("unknown cargo layout?"),
            "{orphan}: {message}"
        );
    }
}
