// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::{
    audit, own_units, own_units_in, path_dependency_units, root_package_with_path_dependencies,
    unit, workspace_below_the_root,
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
        String::from_utf8_lossy(&shared.stderr).contains("so their units cannot be told apart"),
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

#[test]
fn a_unit_of_another_profile_triple_or_kind_does_not_stand_in_for_a_missing_test_harness_unit() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    for unbuilt in [
        format!("{target}/deps/lib-f00000000000000a.d"),
        format!("{target}/.fingerprint/lib-f00000000000000a/test-lib-lib"),
    ] {
        fs::remove_file(repo.root().join(unbuilt)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let stand_ins = [
        ("rust/target/release", "test-lib-lib"),
        ("rust/target/x86_64-unknown-linux-gnu/debug", "test-lib-lib"),
        (target, "lib-lib"),
    ];
    for (configuration, fingerprint) in stand_ins {
        let stand_in = format!("{configuration}/deps/lib-f00000000000000d.d");
        unit(&repo, &stand_in, "out.d: lib/src/lib.rs\n");
        let fingerprint =
            format!("{configuration}/.fingerprint/lib-f00000000000000d/{fingerprint}");
        unit(&repo, &fingerprint, "");
        let partial = audit(&repo, &["--write"]);
        assert_eq!(partial.status.code(), Some(1), "{stand_in}: {partial:?}");
        assert!(
            String::from_utf8_lossy(&partial.stderr).contains(
                " for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs, 1 of 2 units);"
            ),
            "{stand_in}: {partial:?}"
        );
        assert!(!lock.exists());
        for file in [stand_in, fingerprint] {
            fs::remove_file(repo.root().join(file)).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }

    for (configuration, fingerprint) in stand_ins {
        unit(
            &repo,
            &format!("{configuration}/deps/lib-f00000000000000d.d"),
            "out.d: lib/src/lib.rs\n",
        );
        unit(
            &repo,
            &format!("{configuration}/.fingerprint/lib-f00000000000000d/{fingerprint}"),
            "",
        );
    }
    own_units(&repo, target, &[]);
    let partial_triple = audit(&repo, &["--write"]);
    assert_eq!(partial_triple.status.code(), Some(1), "{partial_triple:?}");
    assert!(
        String::from_utf8_lossy(&partial_triple.stderr).contains(
            "/rust/target/x86_64-unknown-linux-gnu/debug for app (rust/app/Cargo.toml) target app (rust/app/src/lib.rs, 0 of 2 units)"
        ),
        "{partial_triple:?}"
    );
    assert!(!lock.exists());

    fs::remove_dir_all(repo.root().join("rust/target/x86_64-unknown-linux-gnu"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");
    assert!(lock.exists());
}

#[test]
fn a_complete_build_for_a_target_triple_with_a_cdylib_and_an_untested_test_target_passes_unless_a_partial_debug_build_remains()
 {
    let repo = workspace_below_the_root();
    repo.write(
        "rust/lib/Cargo.toml",
        "[package]\nname = \"lib\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n[[test]]\nname = \"skipped\"\npath = \"tests/skipped.rs\"\ntest = false\n",
    );
    repo.write("rust/lib/tests/skipped.rs", "");
    let target = "rust/target/x86_64-unknown-linux-gnu/debug";
    own_units_in(&repo, "rust/target/debug", target, &[]);
    fs::rename(
        repo.root()
            .join(format!("{target}/deps/lib-f000000000000005.d")),
        repo.root().join(format!("{target}/deps/lib.d")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        ""
    );

    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    unit(
        &repo,
        "rust/target/check/debug/deps/lib-f00000000000000e.d",
        "out.d: lib/src/lib.rs\n",
    );
    unit(
        &repo,
        "rust/target/check/debug/.fingerprint/lib-f00000000000000e/test-lib-lib",
        "",
    );
    let nested_tool = audit(&repo, &["--write"]);
    assert!(nested_tool.status.success(), "{nested_tool:?}");

    fs::remove_file(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    unit(
        &repo,
        "rust/target/debug/deps/lib-f00000000000000e.d",
        "out.d: lib/src/lib.rs\n",
    );
    unit(
        &repo,
        "rust/target/debug/.fingerprint/lib-f00000000000000e/test-lib-lib",
        "",
    );
    let leftover = audit(&repo, &["--write"]);
    assert_eq!(leftover.status.code(), Some(1), "{leftover:?}");
    assert!(
        String::from_utf8_lossy(&leftover.stderr).contains(
            "/rust/target/debug for app (rust/app/Cargo.toml) target app (rust/app/src/lib.rs, 0 of 2 units)"
        ),
        "{leftover:?}"
    );
    assert!(!lock.exists());
}

#[test]
fn a_reachable_path_dependency_built_without_its_dep_info_makes_the_audit_refuse() {
    let repo = root_package_with_path_dependencies();
    path_dependency_units(&repo);
    let leaf = repo
        .root()
        .join("target/debug/deps/leaf-0000000000000007.d");
    let text = fs::read_to_string(&leaf).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_file(&leaf).unwrap_or_else(|e| unreachable!("{e}"));
    let partial = audit(&repo, &["--write"]);
    assert_eq!(partial.status.code(), Some(1), "{partial:?}");
    assert!(
        String::from_utf8_lossy(&partial.stderr)
            .contains(" for path dependency leaf (vendor/leaf/Cargo.toml, 1 of 2 units), leaf target leaf (normal unit with a cargo fingerprint and no dep-info);"),
        "{partial:?}"
    );
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert!(!lock.exists());

    fs::write(&leaf, text).unwrap_or_else(|e| unreachable!("{e}"));
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert!(lock.exists());
}

#[test]
fn a_target_directory_inside_another_roots_target_directory_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    repo.write(".gitignore", "target/\ntarget-tools/\n");
    own_units(&repo, "rust/target/debug", &[]);
    repo.write(
        "tools/Cargo.toml",
        "[package]\nname = \"tools\"\nedition = \"2024\"\n",
    );
    repo.write("tools/Cargo.lock", "version = 4\n");
    repo.write("tools/src/main.rs", "fn main() {}\n");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    for (directory, holds) in [("rust/target/tools", true), ("rust/target-tools", false)] {
        repo.write(
            "tools/.cargo/config.toml",
            &format!("[build]\ntarget-dir = \"../{directory}\"\n"),
        );
        for (hash, fingerprint) in [
            ("f0000000000000d1", "bin-tools"),
            ("f0000000000000d2", "test-bin-tools"),
        ] {
            unit(
                &repo,
                &format!("{directory}/debug/deps/tools-{hash}.d"),
                "out.d: src/main.rs\n",
            );
            unit(
                &repo,
                &format!("{directory}/debug/.fingerprint/tools-{hash}/{fingerprint}"),
                "",
            );
        }
        let result = audit(&repo, &["--write"]);
        if holds {
            assert_eq!(result.status.code(), Some(1), "{result:?}");
            let complaint = String::from_utf8_lossy(&result.stderr);
            assert!(
                complaint.contains("/rust/target of the Cargo root ")
                    && complaint.contains("holds the target directory ")
                    && complaint.contains("/rust/target/tools of the Cargo root "),
                "{complaint}"
            );
            assert!(!lock.exists());
        } else {
            assert!(result.status.success(), "{result:?}");
            assert!(lock.exists());
        }
    }
}
