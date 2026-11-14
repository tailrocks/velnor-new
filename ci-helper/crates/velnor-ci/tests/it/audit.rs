// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

fn audit(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["inputs", "audit"])
        .args(args)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn unit(repo: &Repo, path: &str, text: &str) {
    let path = repo.root().join(path);
    fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
}

fn workspace_below_the_root() -> Repo {
    let repo = Repo::new("audit");
    repo.write(".gitignore", "target/\n");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "rust/Cargo.toml",
        "[workspace]\nmembers = [\"app\", \"lib\"]\nresolver = \"3\"\n",
    );
    repo.write("rust/Cargo.lock", "version = 4\n");
    for (package, extra) in [
        (
            "app",
            "\n[features]\nextra = []\n\n[[bin]]\nname = \"extra\"\npath = \"src/extra.rs\"\nrequired-features = [\"extra\"]\n",
        ),
        ("lib", "build = \"build.rs\"\n"),
    ] {
        repo.write(
            &format!("rust/{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nedition = \"2024\"\n{extra}"),
        );
        repo.write(&format!("rust/{package}/src/lib.rs"), "");
        repo.write(&format!("rust/{package}/tests/it.rs"), "");
    }
    repo.write("rust/lib/build.rs", "fn main() {}\n");
    repo.write("rust/app/build.rs", "fn main() {}\n");
    repo.write("rust/app/src/extra.rs", "fn main() {}\n");
    repo.write("rust/app/data.txt", "");
    for file in [
        "greeting.txt",
        "app-test.txt",
        "lib-test.txt",
        "build.txt",
        "app-build.txt",
        "old.txt",
    ] {
        repo.write(&format!("shared/{file}"), "");
    }
    repo
}

fn own_units(repo: &Repo, target: &str, left_out: &[&str]) {
    for (key, file, text) in [
        (
            "app",
            "deps/app-f000000000000001.d",
            "out.d: app/src/lib.rs\n",
        ),
        (
            "app-it",
            "deps/it-f000000000000002.d",
            "out.d: app/tests/it.rs\n",
        ),
        (
            "app-build",
            "build/app-f000000000000003/build_script_build-f000000000000003.d",
            "out.d: app/build.rs\n",
        ),
        ("app-build", "build/app-f000000000000004/output", ""),
        (
            "lib",
            "deps/lib-f000000000000005.d",
            "out.d: lib/src/lib.rs\n",
        ),
        (
            "lib-it",
            "deps/it-f000000000000006.d",
            "out.d: lib/tests/it.rs\n",
        ),
        (
            "lib-build",
            "build/lib-f000000000000007/build_script_build-f000000000000007.d",
            "out.d: lib/build.rs\n",
        ),
        ("lib-build", "build/lib-f000000000000008/output", ""),
    ] {
        if !left_out.contains(&key) {
            unit(repo, &format!("{target}/{file}"), text);
        }
    }
}

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
        String::from_utf8_lossy(&partial.stderr)
            .contains(" for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs);"),
        "{partial:?}"
    );
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert!(!lock.exists());

    unit(
        &repo,
        &format!("{target}/deps/lib-0000000000000007.d"),
        "out.d: lib/src/lib.rs\n",
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\nshared/greeting.txt\tapp\n"
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

fn root_package_with_path_dependencies() -> Repo {
    let repo = Repo::new("audit-owners");
    repo.write(".gitignore", "target/\n");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(".velnor/rust-ci.toml", "exclude = [\"vendor\"]\n");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"top\"\nedition = \"2024\"\n\n[[test]]\nname = \"qa\"\npath = \"qa.rs\"\n\n[[test]]\nname = \"ext\"\npath = \"ext.rs\"\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    repo.write(
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nedition = \"2024\"\n\n[[test]]\nname = \"ext\"\npath = \"../ext.rs\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\" }\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    repo.write(
        "vendor/leaf/Cargo.toml",
        "[package]\nname = \"leaf\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    );
    for file in [
        "src/lib.rs",
        "other/src/lib.rs",
        "qa.rs",
        "data/qa.txt",
        "README.md",
        "NOTICE",
        "vendor/mid/src/lib.rs",
        "vendor/mid/build.rs",
        "vendor/leaf/src/lib.rs",
        "vendor/leaf/build.rs",
        "LICENSE",
        "ext.rs",
        "data/ext.txt",
    ] {
        repo.write(file, "");
    }
    fs::create_dir_all(repo.root().join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("../README.md", repo.root().join("shared/link.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn audit_credits_path_dependencies_targets_outside_packages_symlink_targets_and_declared_paths() {
    let repo = root_package_with_path_dependencies();
    let target = "target/debug";
    unit(
        &repo,
        &format!("{target}/deps/top-0000000000000001.d"),
        "out.d: src/lib.rs shared/link.txt\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/other-0000000000000005.d"),
        "out.d: other/src/lib.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/qa-0000000000000002.d"),
        "out.d: qa.rs data/qa.txt\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/mid-0000000000000003.d"),
        "out.d: vendor/mid/src/lib.rs vendor/mid/src/../../../NOTICE\n",
    );
    unit(
        &repo,
        &format!("{target}/build/mid-0000000000000006/build_script_build-0000000000000006.d"),
        "out.d: vendor/mid/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/mid-0000000000000004/output"),
        "cargo:rerun-if-changed=build.rs\ncargo:rerun-if-changed=../../schema/future.json\ncargo:rerun-if-changed=../../target/debug/gen.txt\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/leaf-0000000000000007.d"),
        "out.d: vendor/leaf/src/lib.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/leaf-0000000000000008/build_script_build-0000000000000008.d"),
        "out.d: vendor/leaf/build.rs\n",
    );
    unit(
        &repo,
        &format!("{target}/build/leaf-0000000000000009/output"),
        "cargo:rerun-if-changed=build.rs\ncargo:rerun-if-changed=../../LICENSE\n",
    );
    unit(
        &repo,
        &format!("{target}/deps/ext-000000000000000a.d"),
        "out.d: ext.rs data/ext.txt\n",
    );

    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "LICENSE\tother\nNOTICE\tother\nREADME.md\ttop\ndata/ext.txt\tother\ndata/ext.txt\ttop\ndata/qa.txt\ttop\next.rs\tother\next.rs\ttop\nqa.rs\ttop\nschema/future.json\tother\nshared/link.txt\ttop\n"
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
            .contains("other (other/Cargo.toml) target other (other/src/lib.rs)"),
        "{partial:?}"
    );
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
        String::from_utf8_lossy(&stale_test.stderr)
            .contains(" for lib (rust/lib/Cargo.toml) target it (rust/lib/tests/it.rs);"),
        "{stale_test:?}"
    );

    unit(&repo, &test_unit, "out.d: lib/tests/it.rs\n");
    repo.set_mtime(&format!("{target}/build/lib-f000000000000008/output"), old);
    let stale_output = audit(&repo, &["--write"]);
    assert_eq!(stale_output.status.code(), Some(1), "{stale_output:?}");
    assert!(
        String::from_utf8_lossy(&stale_output.stderr).contains(
            " for lib (rust/lib/Cargo.toml) target build-script-build (rust/lib/build.rs);"
        ),
        "{stale_output:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
