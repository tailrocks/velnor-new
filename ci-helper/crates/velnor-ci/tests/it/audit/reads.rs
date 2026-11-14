// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

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

fn written_lock(repo: &Repo) -> String {
    let written = audit(repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn reads_of_a_path_dependency_whose_sources_lie_outside_its_directory_go_to_the_members_that_reach_it()
 {
    let repo = root_package_with_path_dependencies();
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"../../shared/mid-build.rs\"\n\n[lib]\npath = \"../../shared/mid.rs\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    for file in ["shared/mid.rs", "shared/mid.txt", "shared/mid-build.rs"] {
        repo.write(file, "");
    }
    path_dependency_units(&repo);
    let build = "target/debug/build/mid-0000000000000006/build_script_build-0000000000000006.d";
    unit(&repo, build, "out.d: shared/mid-build.rs\n");
    unit(
        &repo,
        "target/debug/deps/mid-0000000000000003.d",
        "out.d: shared/mid.rs shared/mid.txt\n",
    );
    unit(
        &repo,
        "target/debug/build/mid-0000000000000004/output",
        "cargo:rerun-if-changed=build.rs\ncargo:rerun-if-changed=../../schema/future.json\n",
    );
    assert_eq!(
        written_lock(&repo),
        "LICENSE\tother\nREADME.md\ttop\ndata/ext.txt\tother\ndata/ext.txt\ttop\ndata/qa.txt\ttop\next.rs\tother\next.rs\ttop\nqa.rs\ttop\nschema/future.json\tother\nshared/link.txt\ttop\nshared/mid-build.rs\tother\nshared/mid.rs\tother\nshared/mid.txt\tother\n"
    );
}

#[test]
fn a_package_name_that_several_packages_share_credits_their_reads_to_the_dependents_of_each() {
    let repo = root_package_with_path_dependencies();
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"top\"\nedition = \"2024\"\n\n[[test]]\nname = \"qa\"\npath = \"qa.rs\"\n\n[[test]]\nname = \"ext\"\npath = \"ext.rs\"\n\n[dependencies]\nside = { path = \"vendor/side\", package = \"other\" }\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    repo.write(
        "vendor/side/Cargo.toml",
        "[package]\nname = \"other\"\nversion = \"0.3.0\"\nedition = \"2024\"\n\n[dependencies]\nalt = { path = \"../alt\", package = \"leaf\" }\n",
    );
    repo.write(
        "vendor/alt/Cargo.toml",
        "[package]\nname = \"leaf\"\nversion = \"0.2.0\"\nedition = \"2024\"\n",
    );
    repo.write(
        "vendor/leaf/Cargo.toml",
        "[package]\nname = \"leaf\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[lib]\npath = \"../../shared/leaf.rs\"\n",
    );
    repo.write(
        "vendor/registry/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
    for file in [
        "vendor/alt/src/lib.rs",
        "vendor/registry/mid/src/lib.rs",
        "shared/alt.txt",
        "shared/leaf.rs",
        "shared/leaf.txt",
        "shared/other.txt",
        "shared/third.txt",
    ] {
        repo.write(file, "");
    }
    path_dependency_units(&repo);
    for (hash, package, text) in [
        ("5", "other", "other/src/lib.rs shared/other.txt"),
        ("a1", "leaf", "vendor/alt/src/lib.rs shared/alt.txt"),
        ("7", "leaf", "shared/leaf.rs shared/leaf.txt"),
        (
            "c1",
            "mid",
            "vendor/registry/mid/src/lib.rs shared/third.txt",
        ),
    ] {
        let unit_at = |path: String, text: &str| unit(&repo, &format!("target/debug/{path}"), text);
        unit_at(
            format!("deps/{package}-{hash:0>16}.d"),
            &format!("out.d: {text}\n"),
        );
        unit_at(
            format!(".fingerprint/{package}-{hash:0>16}/lib-{package}"),
            "",
        );
    }
    assert_eq!(
        written_lock(&repo),
        "LICENSE\tother\nLICENSE\ttop\nNOTICE\tother\nREADME.md\ttop\ndata/ext.txt\tother\ndata/ext.txt\ttop\ndata/qa.txt\ttop\next.rs\tother\next.rs\ttop\nother/src/lib.rs\ttop\nqa.rs\ttop\nschema/future.json\tother\nshared/alt.txt\tother\nshared/alt.txt\ttop\nshared/leaf.rs\tother\nshared/leaf.rs\ttop\nshared/leaf.txt\tother\nshared/leaf.txt\ttop\nshared/link.txt\ttop\nshared/other.txt\tother\nshared/other.txt\ttop\nshared/third.txt\tother\nshared/third.txt\ttop\nvendor/alt/src/lib.rs\tother\nvendor/leaf/build.rs\ttop\n"
    );
}
