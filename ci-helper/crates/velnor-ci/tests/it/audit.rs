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
    let root = repo.root().display().to_string();
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
        "shared/app-test.txt\tapp\nshared/build.txt\tlib\nshared/lib-test.txt\tlib\n",
    );
    let stale = audit(&repo, &[]);
    assert_eq!(stale.status.code(), Some(1), "{stale:?}");
    let complaint = String::from_utf8_lossy(&stale.stderr);
    assert!(
        complaint.ends_with("\nshared/greeting.txt\tapp\n"),
        "{complaint}"
    );
}
