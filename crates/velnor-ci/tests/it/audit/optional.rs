// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, UNIX_EPOCH};

use super::{
    audit, own_units, path_dependency_units, root_package_with_path_dependencies, unit,
    workspace_below_the_root,
};

#[test]
fn a_stale_feature_gated_unit_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let extra = format!("{target}/deps/extra-f00000000000000e.d");
    unit(
        &repo,
        &extra,
        "out.d: app/src/extra.rs app/src/../../../shared/greeting.txt\n",
    );
    repo.set_mtime(&extra, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    repo.set_mtime(
        "shared/greeting.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );

    let stale = audit(&repo, &["--write"]);
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_stale_test_false_harness_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    repo.write_all(&[
        (
            "rust/lib/Cargo.toml",
            "[package]\nname = \"lib\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[[test]]\nname = \"skipped\"\npath = \"tests/skipped.rs\"\ntest = false\n",
        ),
        ("rust/lib/tests/skipped.rs", ""),
    ]);
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let skipped = format!("{target}/deps/skipped-f00000000000000e.d");
    unit(
        &repo,
        &skipped,
        "out.d: lib/tests/skipped.rs lib/tests/../../../shared/greeting.txt\n",
    );
    unit(
        &repo,
        &format!("{target}/.fingerprint/lib-f00000000000000e/test-integration-test-skipped"),
        "",
    );
    repo.set_mtime(&skipped, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    repo.set_mtime(
        "shared/greeting.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );

    let stale = audit(&repo, &["--write"]);
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_fresh_unit_does_not_hide_stale_dep_info_for_the_same_source() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let stale_unit = format!("{target}/deps/app-f00000000000000e.d");
    unit(
        &repo,
        &stale_unit,
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    repo.set_mtime(&stale_unit, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    repo.set_mtime(
        "shared/greeting.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );

    let stale = audit(&repo, &["--write"]);
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_required_path_dependency_without_library_evidence_prevents_writing_the_inputs_lock() {
    let repo = root_package_with_path_dependencies();
    path_dependency_units(&repo);
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    for gone in [
        "target/debug/deps/mid-0000000000000003.d",
        "target/debug/.fingerprint/mid-0000000000000003/lib-mid",
    ] {
        std::fs::remove_file(repo.root().join(gone)).unwrap_or_else(|e| unreachable!("{e}"));
    }

    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("for path dependency mid (vendor/mid/Cargo.toml, 1 of 2 units);"),
        "{output:?}"
    );
    assert!(!lock.exists());
}

#[test]
fn an_inactive_optional_path_dependency_without_artifacts_is_accepted() {
    let repo = root_package_with_path_dependencies();
    repo.write_all(&[
        (
            "other/Cargo.toml",
            "[package]\nname = \"other\"\nedition = \"2024\"\n\n[[test]]\nname = \"ext\"\npath = \"../ext.rs\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\", optional = true }\n",
        ),
    ]);
    repo.git(&["add", "--all"]);
    path_dependency_units(&repo);
    for gone in [
        "target/debug/deps/mid-0000000000000003.d",
        "target/debug/.fingerprint/mid-0000000000000003/lib-mid",
        "target/debug/build/mid-0000000000000006/build_script_build-0000000000000006.d",
        "target/debug/.fingerprint/mid-0000000000000006/build-script-build-script-build",
        "target/debug/build/mid-0000000000000004/output",
    ] {
        std::fs::remove_file(repo.root().join(gone)).unwrap_or_else(|e| unreachable!("{e}"));
    }

    let output = audit(&repo, &["--write"]);
    assert!(output.status.success(), "{output:?}");
}
