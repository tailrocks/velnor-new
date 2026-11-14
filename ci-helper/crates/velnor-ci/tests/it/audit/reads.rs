// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::{
    audit, own_units, path_dependency_units, root_package_with_path_dependencies, unit,
    workspace_below_the_root,
};

#[test]
fn audit_writes_reads_outside_each_package_from_current_units_and_fails_on_a_stale_lock() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/deps/app-0000000000000001.d"),
        "out.d: app/src/lib.rs app/data.txt app/src/../../../shared/greeting.txt /elsewhere/src/lib.rs target/debug/build/lib-0000000000000005/out/gen.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/it-0000000000000002.d"),
        "out.d: app/tests/it.rs app/tests/../../../shared/app-test.txt\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/it-0000000000000003.d"),
        "out.d: lib/tests/it.rs lib/tests/../../../shared/lib-test.txt\n",
    );
    let stale = format!("{target}/deps/app-0000000000000004.d");
    unit(&repo, &stale, "out.d: app/src/lib.rs ../shared/old.txt\n");
    repo.set_mtime(&stale, UNIX_EPOCH + Duration::from_secs(1_000_000_000));
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let root = root.display();
    unit(
        &repo,
        &format!("{target}/app.d"),
        &format!("{root}/{target}/app: {root}/rust/app/src/lib.rs {root}/shared/old.txt\n"),
    );
    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000006/build_script_build-0000000000000006.d"),
        "out.d: lib/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000005/output"),
        "cargo:rerun-if-changed=build.rs\ncargo::rerun-if-changed=../../shared/build.txt\ncargo:rustc-env=A=b\n",
    );
    unit(
        &repo,
        &format!("{target}/build/app-000000000000000b/build_script_build-000000000000000b.d"),
        "out.d: app/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/app-000000000000000c/output"),
        "cargo:rerun-if-changed=../../shared/app-build.txt\n",
    );

    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        "shared/app-build.txt\tapp\nshared/app-test.txt\tapp\nshared/build.txt\tlib\nshared/greeting.txt\tapp\nshared/lib-test.txt\tlib\n"
    );

    repo.git(&["add", "--all"]);
    let current = audit(&repo, &[]);
    assert_eq!(current.status.code(), Some(0), "{current:?}");

    repo.write(
        ".velnor/rust-inputs.lock",
        "shared/app-build.txt\tapp\nshared/app-test.txt\tapp\nshared/build.txt\tlib\nshared/lib-test.txt\tlib\nshared/old.txt\tapp\n",
    );
    let stale = audit(&repo, &[]);
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    let complaint = String::from_utf8_lossy(&stale.stderr);
    assert!(
        complaint.ends_with(
            "\nreads it does not list:\nshared/greeting.txt\tapp\nlines nothing reads:\nshared/old.txt\tapp\n"
        ),
        "{complaint}"
    );
}

#[test]
fn a_tracked_path_the_lock_cannot_hold_is_an_error_that_names_it() {
    let repo = workspace_below_the_root();
    repo.write("shared/a\tb.txt", "");
    let unnamed = audit(&repo, &[]);
    assert_eq!(unnamed.status.code(), Some(1), "{unnamed:?}");
    assert!(
        String::from_utf8_lossy(&unnamed.stderr).contains("invalid RepoPath: \"shared/a\\tb.txt\""),
        "{unnamed:?}"
    );
}

#[test]
fn audit_credits_path_dependencies_targets_outside_packages_symlink_targets_and_declared_paths() {
    let repo = root_package_with_path_dependencies();
    path_dependency_units(&repo);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "LICENSE\tother\nNOTICE\tother\nREADME.md\ttop\ndata/ext.txt\tother\ndata/ext.txt\ttop\ndata/qa.txt\ttop\next.rs\tother\next.rs\ttop\nqa.rs\ttop\nschema/future.json\tother\nshared/link.txt\ttop\n"
    );
}

#[test]
fn reads_below_an_unowned_manifest_that_does_not_parse_go_to_every_member_using_unowned_packages() {
    let repo = root_package_with_path_dependencies();
    repo.write("vendor/mid/Cargo.toml", "[package\nname = \"mid\"\n");
    path_dependency_units(&repo);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    let lock = fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(lock.contains("LICENSE\tother\nLICENSE\ttop\n"), "{lock}");
}

#[test]
fn tracked_reads_inside_a_tracked_target_directory_are_kept() {
    let repo = workspace_below_the_root();
    repo.write(
        "rust/.cargo/config.toml",
        "[build]\ntarget-dir = \"../docs\"\n",
    );
    repo.write("docs/schema.json", "");
    let target = "docs/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/deps/app-0000000000000001.d"),
        "out.d: app/src/lib.rs app/src/../../../docs/schema.json\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/lib-0000000000000002.d"),
        "out.d: lib/src/lib.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000003/build_script_build-0000000000000003.d"),
        "out.d: lib/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000004/output"),
        "cargo:rerun-if-changed=../../docs/debug/gen.txt\n",
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "docs/schema.json\tapp\n"
    );
}

#[test]
fn a_built_target_that_needs_features_still_credits_its_reads() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/deps/extra-f00000000000000e.d"),
        "out.d: app/src/extra.rs app/src/../../../shared/greeting.txt\n",
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "shared/greeting.txt\tapp\n"
    );
}
