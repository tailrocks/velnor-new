// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Which build outputs of a plain target a unit owns. The trees are the files a real `cargo build
//! --all-targets` leaves for a workspace with a library, a proc macro, a binary, an example and
//! a package with a build script, in the legacy layout and in the layout of `build-dir-new-layout`.

use std::fs;
use std::path::Path;

use super::layout_tests::target_with;
use super::unit_files_within;

const LEGACY: &[&str] = &[
    "debug/CACHEDIR.TAG",
    "debug/.fingerprint/my-lib-18bfefdf9e43d124/lib-my_lib",
    "debug/.fingerprint/my-lib-8ad54dfc6697efab/test-lib-my_lib",
    "debug/.fingerprint/my-macro-565897ab67399604/lib-my_macro",
    "debug/.fingerprint/my-tool-58b8ba277ce342dc/bin-my-tool",
    "debug/.fingerprint/my-tool-df10fa49aaec42f8/example-tool-ex",
    "debug/.fingerprint/my-app-58267aa43290fa5c/build-script-build-script-build",
    "debug/.fingerprint/my-app-7a7c63f867dc0986/run-build-script-build-script-build",
    "debug/deps/libmy_lib-18bfefdf9e43d124.rlib",
    "debug/deps/libmy_lib-18bfefdf9e43d124.rmeta",
    "debug/deps/my_lib-18bfefdf9e43d124.d",
    "debug/deps/my_lib-8ad54dfc6697efab.d",
    "debug/deps/my_macro-565897ab67399604.d",
    "debug/deps/my_tool-58b8ba277ce342dc.d",
    "debug/examples/tool_ex-df10fa49aaec42f8.d",
    "debug/examples/tool-ex.d",
    "debug/libmy_lib.rlib",
    "debug/libmy_lib.d",
    "debug/libmy_macro.d",
    "debug/my-tool.d",
    "debug/build/my-app-58267aa43290fa5c/build_script_build-58267aa43290fa5c.d",
    "debug/build/my-app-7a7c63f867dc0986/invoked.timestamp",
    "debug/build/my-app-7a7c63f867dc0986/output",
    "debug/build/my-app-7a7c63f867dc0986/out/gen.d",
    "debug/build/my-app-7a7c63f867dc0986/out/generated/object.d",
];

const NEW_LAYOUT: &[&str] = &[
    "debug/CACHEDIR.TAG",
    "debug/build/my-lib/18bfefdf9e43d124/fingerprint/lib-my_lib",
    "debug/build/my-lib/18bfefdf9e43d124/out/libmy_lib-18bfefdf9e43d124.rlib",
    "debug/build/my-lib/18bfefdf9e43d124/out/libmy_lib-18bfefdf9e43d124.rmeta",
    "debug/build/my-lib/18bfefdf9e43d124/out/my_lib-18bfefdf9e43d124.d",
    "debug/build/my-macro/565897ab67399604/fingerprint/lib-my_macro",
    "debug/build/my-macro/565897ab67399604/out/my_macro-565897ab67399604.d",
    "debug/build/my-tool/58b8ba277ce342dc/fingerprint/bin-my-tool",
    "debug/build/my-tool/58b8ba277ce342dc/out/my_tool.d",
    "debug/build/my-tool/df10fa49aaec42f8/fingerprint/example-tool-ex",
    "debug/build/my-tool/df10fa49aaec42f8/out/tool_ex.d",
    "debug/build/my-app/58267aa43290fa5c/out/build_script_build.d",
    "debug/build/my-app/7a7c63f867dc0986/run/stdout",
    "debug/build/my-app/7a7c63f867dc0986/run/invoked.timestamp",
    "debug/build/my-app/7a7c63f867dc0986/out/gen.d",
    "debug/build/my-app/7a7c63f867dc0986/out/generated/object.d",
    "debug/libmy_lib.rlib",
    "debug/libmy_lib.d",
    "debug/libmy_macro.d",
    "debug/my-tool.d",
    "debug/examples/tool-ex.d",
];

const GHOST: &str = "ghost-0123456789abcdef";

fn with_orphan(root: &Path, orphan: &str) -> Result<(), String> {
    let path = root.join(orphan);
    fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&path, "").unwrap_or_else(|e| unreachable!("{e}"));
    let scanned = unit_files_within(root, 1000);
    fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
    scanned.map(|_| ()).map_err(|e| e.to_string())
}

fn assert_refused(root: &Path, orphan: &str) {
    let message = with_orphan(root, orphan).expect_err(orphan);
    assert!(
        message.contains("found no units in") && message.contains("unknown cargo layout?"),
        "{orphan}: {message}"
    );
    assert!(message.contains(orphan.rsplit('/').next().unwrap_or(orphan)));
}

fn in_tree(tree: &[&str], check: impl FnOnce(&Path)) {
    let root = target_with(tree);
    check(&root);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_real_build_in_either_layout_is_not_refused() {
    for tree in [LEGACY, NEW_LAYOUT] {
        let root = target_with(tree);
        let found = unit_files_within(&root, 1000);
        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(found.is_ok(), "{found:?}");
    }
}

#[test]
fn a_hashed_dep_info_file_at_the_target_root_is_an_output_no_unit_owns() {
    for tree in [LEGACY, NEW_LAYOUT] {
        in_tree(tree, |root| {
            assert_refused(root, &format!("{GHOST}.d"));
            assert_refused(root, &format!("debug/{GHOST}.d"));
        });
    }
}

#[test]
fn a_dep_info_file_below_a_folder_of_a_build_script_out_dir_is_generated_content() {
    for (tree, out) in [
        (LEGACY, "debug/build/my-app-7a7c63f867dc0986/out"),
        (NEW_LAYOUT, "debug/build/my-app/7a7c63f867dc0986/out"),
    ] {
        in_tree(tree, |root| {
            for nested in [
                "generated/object.d",
                "generated/deep/object.d",
                "generated/deps/dep-0123456789abcdef.d",
                "generated/libgen.rlib",
                "deps/dep-0123456789abcdef.d",
            ] {
                let accepted = with_orphan(root, &format!("{out}/{nested}"));
                assert!(accepted.is_ok(), "{out}/{nested}: {accepted:?}");
            }
        });
    }
}

#[test]
fn a_nested_out_dir_file_is_not_read_as_a_unit() {
    let root = target_with(&[
        "debug/build/dep/33cec232bab789d1/run/stdout",
        "debug/build/dep/33cec232bab789d1/out/deps/gen-0123456789abcdef.d",
        "debug/build/dep-33cec232bab789d1/output",
        "debug/build/dep-33cec232bab789d1/out/deps/gen-0123456789abcdef.d",
    ]);
    let found = unit_files_within(&root, 100).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn an_output_of_a_crate_no_unit_builds_is_refused_in_every_place_cargo_writes_one() {
    in_tree(LEGACY, |root| {
        for orphan in [
            "debug/libghost.rlib",
            "debug/libghost.d",
            "debug/ghost.d",
            "debug/examples/ghost.d",
            "debug/examples/libghost.rlib",
            "debug/deps/libghost-0123456789abcdef.rlib",
            "debug/deps/libghost-0123456789abcdef.rmeta",
            "other/libmy_lib.rlib",
        ] {
            assert_refused(root, orphan);
        }
    });
    in_tree(NEW_LAYOUT, |root| {
        for orphan in [
            "debug/libghost.rlib",
            "debug/ghost.d",
            "debug/examples/ghost.d",
            "debug/build/ghost/0123456789abcdef/out/libghost-0123456789abcdef.rlib",
            "debug/build/my-lib/0123456789abcdef/out/libghost-0123456789abcdef.rmeta",
            "other/libmy_lib.rlib",
        ] {
            assert_refused(root, orphan);
        }
    });
}

#[test]
fn a_profile_named_like_a_cargo_folder_keeps_its_own_outputs_apart() {
    for profile in ["deps", "examples", "out", "build", "fingerprint"] {
        let tree: Vec<String> = LEGACY
            .iter()
            .chain(NEW_LAYOUT)
            .map(|file| file.replacen("debug/", &format!("{profile}/"), 1))
            .collect();
        let tree: Vec<&str> = tree.iter().map(String::as_str).collect();
        in_tree(&tree, |root| {
            let found = unit_files_within(root, 1000);
            assert!(found.is_ok(), "{profile}: {found:?}");
            for orphan in [
                "libghost.rlib",
                "ghost-0123456789abcdef.d",
                "examples/libghost.rlib",
                "deps/libghost-0123456789abcdef.rlib",
            ] {
                assert_refused(root, &format!("{profile}/{orphan}"));
            }
        });
    }
}
