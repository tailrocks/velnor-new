// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::owner_tests::{bin_only_build, example_build, scanned};

const PROFILES: [&str; 5] = ["deps", "incremental", "out", "fingerprint", "examples"];

const ORPHANS: [&str; 8] = [
    "libzzz.rlib",
    "zzz.d",
    "libzzz.d",
    "libapp.rlib",
    "invoked.timestamp",
    "deps/libzzz.rlib",
    "examples/libzzz.rlib",
    "examples/zzz.d",
];

const FINAL: [&str; 1] = ["libmy_lib.rlib"];

const LEGACY: [&str; 15] = [
    ".fingerprint/my-lib-7a59b5c13f7d6e92/lib-my_lib",
    ".fingerprint/my-lib-7a59b5c13f7d6e92/invoked.timestamp",
    ".fingerprint/my-tool-22776aabf1eb61be/bin-my-tool",
    ".fingerprint/my-tool-22776aabf1eb61be/invoked.timestamp",
    "deps/libmy_lib-7a59b5c13f7d6e92.rlib",
    "deps/libmy_lib-7a59b5c13f7d6e92.rmeta",
    "deps/my_lib-7a59b5c13f7d6e92.d",
    "deps/my_tool-22776aabf1eb61be.d",
    "deps/my_tool-22776aabf1eb61be",
    "examples/tool_ex-27aa01cd7d367154.d",
    "examples/tool-ex.d",
    "examples/tool_ex-27aa01cd7d367154",
    "libmy_lib.rlib",
    "libmy_lib.d",
    "my-tool.d",
];

fn current(profile: &str) -> Vec<String> {
    let mut files = bin_only_build();
    files.extend(example_build());
    files.extend(["debug/libmy_lib.rlib", "debug/libmy_lib.d"].map(str::to_owned));
    files.retain(|file| !file.starts_with("debug/CACHEDIR"));
    files.sort();
    files.dedup();
    files
        .into_iter()
        .map(|file| file.replacen("debug/", &format!("{profile}/"), 1))
        .collect()
}

fn legacy(profile: &str) -> Vec<String> {
    LEGACY
        .iter()
        .map(|file| format!("{profile}/{file}"))
        .collect()
}

fn layouts(profile: &str) -> [(&'static str, Vec<String>); 2] {
    [("current", current(profile)), ("legacy", legacy(profile))]
}

#[test]
fn a_profile_named_like_a_cargo_folder_owns_its_final_artifacts() {
    for profile in PROFILES {
        for (layout, files) in layouts(profile) {
            scanned(&files).unwrap_or_else(|e| unreachable!("{profile} {layout}: {e}"));
        }
    }
}

#[test]
fn a_profile_named_like_a_cargo_folder_still_refuses_an_output_no_unit_owns() {
    for profile in PROFILES {
        for (layout, files) in layouts(profile) {
            for orphan in ORPHANS {
                let mut planted = files.clone();
                planted.push(format!("{profile}/{orphan}"));
                let message =
                    scanned(&planted).expect_err(&format!("{profile} {layout} {orphan}: accepted"));
                assert!(
                    message.contains("unknown cargo layout?"),
                    "{profile} {layout} {orphan}: {message}"
                );
            }
        }
    }
}

#[test]
fn the_final_artifacts_of_a_profile_are_not_owned_through_its_parent_directory() {
    for orphan in FINAL
        .map(|file| format!("debug/deps/{file}"))
        .into_iter()
        .chain([
            "debug/build/libmy_lib.rlib".to_owned(),
            "debug/build/libmy_lib.d".to_owned(),
            "debug/zzz/libmy_lib.rlib".to_owned(),
            "debug/zzz/my-tool.d".to_owned(),
        ])
    {
        let mut files = bin_only_build();
        files.push(orphan.clone());
        let message = scanned(&files).expect_err(&format!("{orphan}: accepted"));
        assert!(
            message.contains("unknown cargo layout?"),
            "{orphan}: {message}"
        );
    }
}
