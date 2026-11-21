// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};
use crate::io::Repo;

const SECRET_LINE: &str = "# https://x-access-token:ghs_secret@github.com/acme/shared-lib\n";

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["-c", "user.name=lab", "-c", "user.email=lab@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn cargo(directory: &Path, home: &Path, target: &Path, offline: bool, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO"));
    command
        .current_dir(directory)
        .args(args)
        .env_clear()
        .env("CARGO_HOME", home.join(".cargo"))
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_NET_OFFLINE", offline.to_string());
    for name in ["PATH", "HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "TMPDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "cargo {args:?}: {output:?}");
    output
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|e| unreachable!("{e}"));
    for entry in fs::read_dir(from).unwrap_or_else(|e| unreachable!("{e}")) {
        let entry = entry.unwrap_or_else(|e| unreachable!("{e}"));
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
}

fn append(path: &Path, text: &str) {
    let mut content = fs::read_to_string(path).unwrap_or_default();
    content.push_str(text);
    fs::write(path, content).unwrap_or_else(|e| unreachable!("{e}"));
}

fn dependency(work: &Path) -> (PathBuf, String) {
    let source = work.join("dep-source");
    fs::create_dir_all(source.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        source.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        source.join("src/lib.rs"),
        "pub fn answer() -> u32 {\n    42\n}\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    git(&source, &["init", "--quiet", "--object-format=sha1"]);
    git(&source, &["add", "--all"]);
    git(&source, &["commit", "--quiet", "--message", "dep"]);
    let sha = git(&source, &["rev-parse", "HEAD"]);
    let bare = work.join("dep.git");
    git(
        work,
        &["clone", "--quiet", "--bare", "dep-source", "dep.git"],
    );
    (bare, sha)
}

fn application(work: &Path, bare: &Path) -> PathBuf {
    let app = work.join("app");
    fs::create_dir_all(app.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        app.join("Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndep = {{ git = \"file://{}\" }}\n",
            bare.display()
        ),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        app.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", dep::answer());\n}\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    app
}

fn single_database(home: &Path) -> PathBuf {
    let databases = home.join(".cargo/git/db");
    let mut found: Vec<PathBuf> = fs::read_dir(&databases)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")).path())
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

fn restored_database(work: &Path, saved: &Path, name: &str) -> PathBuf {
    let home = work.join(name);
    let database = home.join(".cargo/git/db").join(
        saved
            .file_name()
            .unwrap_or_else(|| unreachable!("a database has a name")),
    );
    copy_tree(saved, &database);
    git(&database, &["fsck", "--full", "--strict"]);
    home
}

#[test]
fn a_restored_git_database_without_hooks_fetch_head_or_logs_still_serves_real_cargo() {
    let scratch = Repo::empty("cargo-proof");
    let work = scratch.root();
    let (bare, sha) = dependency(work);
    let app = application(work, &bare);
    let seed = work.join("seed");
    cargo(
        &app,
        &seed,
        &work.join("seed-target"),
        false,
        &["generate-lockfile"],
    );
    let repo = private_checkout("cargo-proof-decide", GIT_LOCK);
    let home = repo.root().join("home");
    cargo(
        &app,
        &home,
        &work.join("fetch-target"),
        false,
        &["fetch", "--locked"],
    );
    let database = single_database(&home);
    git(
        &database,
        &[
            "fetch",
            "--quiet",
            &format!("file://{}", bare.display()),
            "HEAD",
        ],
    );
    git(
        &database,
        &[
            "-c",
            "core.logAllRefUpdates=true",
            "update-ref",
            "refs/heads/planted",
            &sha,
        ],
    );
    fs::create_dir_all(database.join("hooks")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(database.join("hooks/post-fetch"), SECRET_LINE)
        .unwrap_or_else(|e| unreachable!("{e}"));
    append(&database.join("FETCH_HEAD"), SECRET_LINE);
    append(&database.join("logs/refs/heads/planted"), SECRET_LINE);
    for planted in ["hooks", "FETCH_HEAD", "logs/refs/heads/planted"] {
        assert!(database.join(planted).exists(), "{planted}");
    }

    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    for left_out in ["hooks", "FETCH_HEAD", "logs"] {
        assert!(!database.join(left_out).exists(), "{left_out}");
    }

    let offline = restored_database(work, &database, "restored-offline");
    let built = cargo(
        &app,
        &offline,
        &work.join("offline-target"),
        true,
        &["build", "--locked"],
    );
    assert!(
        String::from_utf8_lossy(&built.stderr).contains("Finished"),
        "{built:?}"
    );
    let online = restored_database(work, &database, "restored-online");
    cargo(
        &app,
        &online,
        &work.join("online-target"),
        false,
        &["fetch", "--locked"],
    );
    cargo(
        &app,
        &online,
        &work.join("online-target"),
        false,
        &["build", "--locked"],
    );
    let ran = Command::new(work.join("online-target/debug/app"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "42");
}
