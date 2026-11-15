// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

use super::{
    audit, own_units, path_dependency_units, path_dependency_units_in,
    root_package_with_path_dependencies, unit, workspace_below_the_root,
};

fn refuses(output: &Output, text: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(text),
        "{output:?}"
    );
}

const TOOLS: [(&str, &str, &str); 4] = [
    ("f000000000000011", "first_tool", "bin-first-tool"),
    ("f000000000000012", "first_tool", "test-bin-first-tool"),
    ("f000000000000014", "second_tool", "test-bin-second-tool"),
    ("f000000000000013", "second_tool", "bin-second-tool"),
];

fn app_with_two_tools(built: usize) -> Repo {
    let repo = workspace_below_the_root();
    repo.write_all(&[
        (
            "rust/app/Cargo.toml",
            "[package]\nname = \"app\"\nedition = \"2024\"\n\n[[bin]]\nname = \"first-tool\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"second-tool\"\npath = \"src/main.rs\"\n",
        ),
        ("rust/app/src/main.rs", "fn main() {}\n"),
    ]);
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/.fingerprint/serde-f0000000000000ee/lib-serde"),
        "",
    );
    for (hash, name, fingerprint) in &TOOLS[..built] {
        unit(
            &repo,
            &format!("{target}/deps/{name}-{hash}.d"),
            "out.d: app/src/main.rs\n",
        );
        unit(
            &repo,
            &format!("{target}/.fingerprint/app-{hash}/{fingerprint}"),
            "",
        );
    }
    repo
}

#[test]
fn a_target_of_an_app_binary_without_dep_info_makes_the_audit_refuse() {
    let repo = app_with_two_tools(3);
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    refuses(
        &audit(&repo, &["--write"]),
        " for app (rust/app/Cargo.toml) target second-tool (rust/app/src/main.rs, 1 of 2 units);",
    );
    assert!(!lock.exists());

    let target = "rust/target/debug";
    unit(
        &repo,
        &format!("{target}/deps/second_tool-f000000000000013.d"),
        "out.d: app/src/main.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/.fingerprint/app-f000000000000013/bin-second-tool"),
        "",
    );
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");
    assert!(lock.exists());
}

#[test]
fn a_feature_variant_of_a_test_harness_without_dep_info_makes_the_audit_refuse() {
    let repo = app_with_two_tools(4);
    let target = "rust/target/debug";
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    unit(
        &repo,
        &format!("{target}/.fingerprint/lib-f00000000000000e/test-lib-lib"),
        "",
    );
    refuses(
        &audit(&repo, &["--write"]),
        " lib target lib (test harness unit with a cargo fingerprint and no dep-info);",
    );
    assert!(!lock.exists());

    unit(
        &repo,
        &format!("{target}/deps/lib-f00000000000000e.d"),
        "out.d: lib/src/lib.rs\n",
    );
    let variant = audit(&repo, &["--write"]);
    assert!(variant.status.success(), "{variant:?}");
    assert!(lock.exists());
}

#[test]
fn a_path_dependency_needs_its_build_script_run_and_a_current_unit() {
    let repo = root_package_with_path_dependencies();
    path_dependency_units(&repo);
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let output = repo
        .root()
        .join("target/debug/build/leaf-0000000000000009/output");
    let text = fs::read_to_string(&output).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_file(&output).unwrap_or_else(|e| unreachable!("{e}"));
    refuses(
        &audit(&repo, &["--write"]),
        " for path dependency leaf (vendor/leaf/Cargo.toml, 1 of 2 units);",
    );
    assert!(!lock.exists());

    let compiled = "target/debug/build/leaf-0000000000000008/build_script_build-0000000000000008.d";
    repo.set_mtime(compiled, UNIX_EPOCH + Duration::from_secs(1_000_000_000));
    refuses(
        &audit(&repo, &["--write"]),
        " for path dependency leaf (vendor/leaf/Cargo.toml, 1 of 2 units);",
    );
    assert!(!lock.exists());
    unit(&repo, compiled, "out.d: vendor/leaf/build.rs\n");
    fs::write(&output, text).unwrap_or_else(|e| unreachable!("{e}"));

    repo.write_all(&[
        (
            "vendor/side/Cargo.toml",
            "[package]\nname = \"side\"\nedition = \"2024\"\n\n[dependencies]\nalt = { path = \"../alt\", package = \"leaf\", optional = true }\n",
        ),
        (
            "vendor/alt/Cargo.toml",
            "[package]\nname = \"leaf\"\nedition = \"2024\"\n",
        ),
        ("vendor/alt/src/lib.rs", ""),
    ]);
    let unbuilt = audit(&repo, &["--write"]);
    assert!(unbuilt.status.success(), "{unbuilt:?}");
    assert!(lock.exists());
}

#[test]
fn a_path_dependency_built_under_a_target_triple_needs_a_current_unit_per_package() {
    let repo = root_package_with_path_dependencies();
    repo.write_all(&[
        (
            "vendor/side/Cargo.toml",
            "[package]\nname = \"side\"\nedition = \"2024\"\n\n[dependencies]\nalt = { path = \"../alt\", package = \"leaf\", optional = true }\n",
        ),
        (
            "vendor/alt/Cargo.toml",
            "[package]\nname = \"leaf\"\nedition = \"2024\"\n",
        ),
        ("vendor/alt/src/lib.rs", ""),
    ]);
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let triple = "target/x86_64-unknown-linux-gnu/debug";
    path_dependency_units_in(&repo, triple);
    unit(
        &repo,
        "target/debug/deps/leaf-000000000000001f.d",
        "out.d: vendor/leaf/src/lib.rs\n",
    );
    unit(
        &repo,
        "target/debug/.fingerprint/leaf-000000000000001f/lib-leaf",
        "",
    );
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");
    assert!(lock.exists());
    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    repo.set_mtime(
        &format!("{triple}/deps/leaf-0000000000000007.d"),
        UNIX_EPOCH + Duration::from_secs(1_000_000_000),
    );
    refuses(
        &audit(&repo, &["--write"]),
        "/target/x86_64-unknown-linux-gnu/debug for path dependency leaf (vendor/alt/Cargo.toml or vendor/leaf/Cargo.toml, 2 of 3 units);",
    );
    assert!(!lock.exists());
}
