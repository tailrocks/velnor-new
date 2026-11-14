// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use super::{
    audit, own_units, own_units_in, path_dependency_units, path_dependency_units_in,
    root_package_with_path_dependencies, unit, workspace_below_the_root,
};

fn refuses(output: &Output, text: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(text),
        "{output:?}"
    );
}

#[test]
fn a_unit_of_another_target_or_a_feature_variant_without_dep_info_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    repo.write(
        "rust/app/Cargo.toml",
        "[package]\nname = \"app\"\nedition = \"2024\"\n\n[[bin]]\nname = \"first-tool\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"second-tool\"\npath = \"src/main.rs\"\n",
    );
    repo.write("rust/app/src/main.rs", "fn main() {}\n");
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/.fingerprint/serde-f0000000000000ee/lib-serde"),
        "",
    );
    let built = |hash: &str, name: &str, fingerprint: &str| {
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
    };
    built("f000000000000011", "first_tool", "bin-first-tool");
    built("f000000000000012", "first_tool", "test-bin-first-tool");
    built("f000000000000014", "second_tool", "test-bin-second-tool");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    refuses(
        &audit(&repo, &["--write"]),
        " for app (rust/app/Cargo.toml) target second-tool (rust/app/src/main.rs, 1 of 2 units);",
    );
    assert!(!lock.exists());

    built("f000000000000013", "second_tool", "bin-second-tool");
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");
    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));

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
}

#[test]
fn a_path_dependency_needs_its_build_script_run_and_a_current_unit_wherever_cargo_built_it() {
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

    repo.write(
        "vendor/side/Cargo.toml",
        "[package]\nname = \"side\"\nedition = \"2024\"\n\n[dependencies]\nalt = { path = \"../alt\", package = \"leaf\", optional = true }\n",
    );
    repo.write(
        "vendor/alt/Cargo.toml",
        "[package]\nname = \"leaf\"\nedition = \"2024\"\n",
    );
    repo.write("vendor/alt/src/lib.rs", "");
    let unbuilt = audit(&repo, &["--write"]);
    assert!(unbuilt.status.success(), "{unbuilt:?}");
    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));

    fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
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

#[test]
fn a_path_dependency_whose_library_and_build_script_live_outside_its_directory_passes() {
    let repo = root_package_with_path_dependencies();
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"../../shared/mid-build.rs\"\n\n[lib]\npath = \"../../shared/mid.rs\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    repo.write("shared/mid.rs", "");
    repo.write("shared/mid-build.rs", "fn main() {}\n");
    path_dependency_units(&repo);
    let target = "target/debug";
    unit(
        &repo,
        &format!("{target}/deps/mid-0000000000000003.d"),
        "out.d: shared/mid.rs NOTICE\n",
    );
    unit(
        &repo,
        &format!("{target}/build/mid-0000000000000006/build_script_build-0000000000000006.d"),
        "out.d: shared/mid-build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/mid-0000000000000004/output"),
        "cargo:rerun-if-changed=build.rs\n",
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
}

#[test]
fn a_rustc_that_cannot_list_its_targets_makes_the_audit_refuse_and_leaves_the_lock() {
    let repo = workspace_below_the_root();
    own_units_in(
        &repo,
        "rust/target/debug",
        "rust/target/x86_64-unknown-linux-gnu/debug",
        &[],
    );
    let listed = "shared/old.txt\tapp\n";
    repo.write(".velnor/rust-inputs.lock", listed);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env("RUSTC", repo.root().join("missing-rustc"))
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    refuses(&output, "cannot run rustc --print target-list");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        listed
    );
}
