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
    for (package, extra) in [("app", ""), ("lib", "build = \"build.rs\"\n")] {
        repo.write(
            &format!("rust/{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nedition = \"2024\"\n{extra}"),
        );
        repo.write(&format!("rust/{package}/src/lib.rs"), "");
        repo.write(&format!("rust/{package}/tests/it.rs"), "");
    }
    repo.write("rust/lib/build.rs", "fn main() {}\n");
    repo.write("rust/app/data.txt", "");
    for file in [
        "greeting.txt",
        "app-test.txt",
        "lib-test.txt",
        "build.txt",
        "old.txt",
    ] {
        repo.write(&format!("shared/{file}"), "");
    }
    repo
}

#[test]
fn audit_writes_reads_outside_each_package_from_current_units_and_fails_on_a_stale_lock() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
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
        &format!("{target}/build/lib-0000000000000005/output"),
        "cargo:rerun-if-changed=build.rs\ncargo::rerun-if-changed=../../shared/build.txt\ncargo:rustc-env=A=b\n",
    );

    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        "shared/app-test.txt\tapp\nshared/build.txt\tlib\nshared/greeting.txt\tapp\nshared/lib-test.txt\tlib\n"
    );

    repo.git(&["add", "--all"]);
    let current = audit(&repo, &[]);
    assert_eq!(current.status.code(), Some(0), "{current:?}");

    repo.write(
        ".velnor/rust-inputs.lock",
        "shared/app-test.txt\tapp\nshared/build.txt\tlib\nshared/lib-test.txt\tlib\nshared/old.txt\tapp\n",
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
    unit(
        &repo,
        &format!("{target}/deps/app-0000000000000001.d"),
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    let partial = audit(&repo, &["--write"]);
    assert_eq!(partial.status.code(), Some(1), "{partial:?}");
    assert!(
        String::from_utf8_lossy(&partial.stderr).contains(" for lib (rust/lib/Cargo.toml);"),
        "{partial:?}"
    );
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert!(!lock.exists());

    unit(
        &repo,
        &format!("{target}/build/lib-0000000000000005/output"),
        "cargo:rerun-if-changed=../..\n",
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    let lines = fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    for line in [
        "shared/old.txt\tlib\n",
        "rust/app/src/lib.rs\tlib\n",
        "mise.toml\tlib\n",
    ] {
        assert!(lines.contains(line), "{lines}");
    }
    assert!(!lines.contains("rust/lib/"), "{lines}");
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
fn audit_credits_path_dependencies_custom_targets_symlink_targets_and_declared_paths() {
    let repo = Repo::new("audit-owners");
    repo.write(".gitignore", "target/\n");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(".velnor/rust-ci.toml", "exclude = [\"vendor/mid\"]\n");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"top\"\nedition = \"2024\"\n\n[[test]]\nname = \"qa\"\npath = \"qa.rs\"\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    repo.write(
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nedition = \"2024\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\" }\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
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
    ] {
        repo.write(file, "");
    }
    fs::create_dir_all(repo.root().join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("../README.md", repo.root().join("shared/link.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
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
        &format!("{target}/build/mid-0000000000000004/output"),
        "cargo:rerun-if-changed=build.rs\ncargo:rerun-if-changed=../../schema/future.json\ncargo:rerun-if-changed=../../target/debug/gen.txt\n",
    );

    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "NOTICE\tother\nREADME.md\ttop\ndata/qa.txt\ttop\nqa.rs\ttop\nschema/future.json\tother\nshared/link.txt\ttop\n"
    );
}
