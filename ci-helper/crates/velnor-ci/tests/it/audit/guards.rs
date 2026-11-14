// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::{
    audit, own_units, root_package_with_path_dependencies, unit, workspace_below_the_root,
};

#[test]
fn audit_refuses_a_package_without_a_current_unit_and_counts_a_read_of_the_whole_repository() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &["lib"]);
    unit(
        &repo,
        &format!("{target}/deps/app-0000000000000001.d"),
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000006/build_script_build-0000000000000006.d"),
        "out.d: lib/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000005/output"),
        "cargo:rerun-if-changed=../..\n",
    );
    let partial = audit(&repo, &["--write"]);
    assert_eq!(partial.status.code(), Some(1), "{partial:?}");
    assert!(
        String::from_utf8_lossy(&partial.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs, 0 of 2 units);"
        ),
        "{partial:?}"
    );
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert!(!lock.exists());

    own_units(
        &repo,
        target,
        &["app", "app-it", "app-build", "lib-it", "lib-build"],
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\nshared/greeting.txt\tapp\n"
    );
}

#[test]
fn a_path_dependency_unit_does_not_stand_in_for_the_member_that_depends_on_it() {
    let repo = root_package_with_path_dependencies();
    let target = "target/debug";
    unit(
        &repo,
        &format!("{target}/deps/top-0000000000000001.d"),
        "out.d: src/lib.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/mid-0000000000000003.d"),
        "out.d: vendor/mid/src/lib.rs\n",
    );
    let partial = audit(&repo, &["--write"]);
    assert_eq!(partial.status.code(), Some(1), "{partial:?}");
    assert!(
        String::from_utf8_lossy(&partial.stderr)
            .contains("other (other/Cargo.toml) target other (other/src/lib.rs, 0 of 2 units)"),
        "{partial:?}"
    );
}

#[test]
fn a_stale_target_unit_or_a_build_output_older_than_its_build_script_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let old = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let test_unit = format!("{target}/deps/it-f000000000000006.d");
    repo.set_mtime(&test_unit, old);
    let stale_test = audit(&repo, &["--write"]);
    assert_eq!(stale_test.status.code(), Some(1), "{stale_test:?}");
    assert!(
        String::from_utf8_lossy(&stale_test.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target it (rust/lib/tests/it.rs, 0 of 1 units);"
        ),
        "{stale_test:?}"
    );

    unit(&repo, &test_unit, "out.d: lib/tests/it.rs\n");
    repo.set_mtime(&format!("{target}/build/lib-f000000000000008/output"), old);
    let stale_output = audit(&repo, &["--write"]);
    assert_eq!(stale_output.status.code(), Some(1), "{stale_output:?}");
    assert!(
        String::from_utf8_lossy(&stale_output.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target build-script-build (rust/lib/build.rs, 0 of 1 units);"
        ),
        "{stale_output:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_library_without_its_test_harness_unit_or_a_shared_target_directory_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    repo.set_mtime(
        &format!("{target}/deps/lib-f00000000000000a.d"),
        UNIX_EPOCH + Duration::from_secs(1_000_000_000),
    );
    let harness = audit(&repo, &["--write"]);
    assert_eq!(harness.status.code(), Some(1), "{harness:?}");
    assert!(
        String::from_utf8_lossy(&harness.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs, 1 of 2 units);"
        ),
        "{harness:?}"
    );

    repo.write(
        "rust/.cargo/config.toml",
        "[build]\ntarget-dir = \"../shared\"\n",
    );
    repo.write(
        "tools/.cargo/config.toml",
        "[build]\ntarget-dir = \"../shared\"\n",
    );
    repo.write(
        "tools/Cargo.toml",
        "[package]\nname = \"tools\"\nedition = \"2024\"\n",
    );
    repo.write("tools/Cargo.lock", "version = 4\n");
    repo.write("tools/src/main.rs", "fn main() {}\n");
    let shared = audit(&repo, &["--write"]);
    assert_eq!(shared.status.code(), Some(1), "{shared:?}");
    assert!(
        String::from_utf8_lossy(&shared.stderr)
            .contains("two Cargo roots share the target directory"),
        "{shared:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_build_output_whose_declared_input_changed_since_leaves_the_build_script_uncovered() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &["lib-build"]);
    unit(
        &repo,
        &format!("{target}/build/lib-000000000000000e/build_script_build-000000000000000e.d"),
        "out.d: lib/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/lib-000000000000000f/output"),
        "cargo:rerun-if-changed=../../shared/build.txt\n",
    );
    repo.set_mtime(
        "shared/build.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let changed = audit(&repo, &["--write"]);
    assert_eq!(changed.status.code(), Some(1), "{changed:?}");
    assert!(
        String::from_utf8_lossy(&changed.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target build-script-build (rust/lib/build.rs, 0 of 1 units);"
        ),
        "{changed:?}"
    );
}
