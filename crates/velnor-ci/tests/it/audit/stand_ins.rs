// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;

use super::{audit, own_units, unit, workspace_below_the_root};

const STAND_INS: [(&str, &str); 3] = [
    ("rust/target/release", "test-lib-lib"),
    ("rust/target/x86_64-unknown-linux-gnu/debug", "test-lib-lib"),
    ("rust/target/debug", "lib-lib"),
];

fn without_the_lib_test_harness_unit() -> Repo {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    for unbuilt in [
        format!("{target}/deps/lib-f00000000000000a.d"),
        format!("{target}/.fingerprint/lib-f00000000000000a/test-lib-lib"),
    ] {
        fs::remove_file(repo.root().join(unbuilt)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    repo
}

fn place_stand_ins(repo: &Repo, stand_ins: &[(&str, &str)]) {
    for (configuration, fingerprint) in stand_ins {
        unit(
            repo,
            &format!("{configuration}/deps/lib-f00000000000000d.d"),
            "out.d: lib/src/lib.rs\n",
        );
        unit(
            repo,
            &format!("{configuration}/.fingerprint/lib-f00000000000000d/{fingerprint}"),
            "",
        );
    }
}

fn a_stand_in_does_not_cover_the_missing_unit(stand_in: (&str, &str)) {
    let repo = without_the_lib_test_harness_unit();
    place_stand_ins(&repo, &[stand_in]);
    let partial = audit(&repo, &["--write"]);
    assert_eq!(partial.status.code(), Some(1), "{stand_in:?}: {partial:?}");
    assert!(
        String::from_utf8_lossy(&partial.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs, 1 of 2 units);"
        ),
        "{stand_in:?}: {partial:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_test_harness_unit_of_the_release_profile_does_not_stand_in_for_a_missing_one() {
    a_stand_in_does_not_cover_the_missing_unit(STAND_INS[0]);
}

#[test]
fn a_test_harness_unit_of_another_triple_does_not_stand_in_for_a_missing_one() {
    a_stand_in_does_not_cover_the_missing_unit(STAND_INS[1]);
}

#[test]
fn a_library_unit_of_another_kind_does_not_stand_in_for_a_missing_test_harness_unit() {
    a_stand_in_does_not_cover_the_missing_unit(STAND_INS[2]);
}

#[test]
fn a_partial_build_for_a_target_triple_refuses_until_that_triple_is_removed() {
    let repo = without_the_lib_test_harness_unit();
    place_stand_ins(&repo, &STAND_INS);
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    unit(
        &repo,
        "rust/target/x86_64-unknown-linux-gnu/CACHEDIR.TAG",
        "",
    );
    own_units(&repo, "rust/target/debug", &[]);
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
