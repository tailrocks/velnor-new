// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

<<<<<<< HEAD
use std::fs;

use super::{command, own_units, workspace_below_the_root};
use velnor_test_support::real_cargo_str;

#[test]
fn every_metadata_probe_runs_under_the_toolchain_the_command_selects() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    repo.write_unstaged(
        "driver/cargo.rs",
        r#"use std::io::Write;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "metadata") {
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var("PROBE_LOG").unwrap()).unwrap();
        writeln!(log, "{}", args.first().unwrap()).unwrap();
        let rest: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('+')).collect();
        std::process::exit(std::process::Command::new("@CARGO@").args(rest).status().unwrap().code().unwrap_or(1));
    }
}
"#
        .replace("@CARGO@", &real_cargo_str())
        .as_str(),
    );
    let compiled = command("rustc")
        .current_dir(repo.root())
        .args(["--edition=2024", "driver/cargo.rs", "-o", "driver/cargo"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(compiled.status.success(), "{compiled:?}");
    fs::create_dir_all(repo.root().join("driver/path")).unwrap_or_else(|error| unreachable!("{error}"));
    fs::copy(
        repo.root().join("driver/cargo"),
        repo.root().join("driver/path/cargo"),
    )
    .unwrap_or_else(|error| unreachable!("{error}"));
    let path = std::env::join_paths(
        std::iter::once(repo.root().join("driver/path"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap_or_else(|error| unreachable!("{error}"));
    let log = repo.root().join("probes.log");
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .env("PATH", path)
        .env("PROBE_LOG", &log)
        .args(["inputs", "build", "--"])
        .arg(repo.root().join("driver/cargo"))
        .args(["+nightly", "build", "--workspace"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    let probes = fs::read_to_string(&log).unwrap_or_else(|error| unreachable!("{error} {built:?}"));
    let first_words: Vec<&str> = probes.lines().collect();
    assert!(!first_words.is_empty(), "{built:?}");
    assert!(
        first_words.iter().all(|word| *word == "+nightly"),
        "{first_words:?} {built:?}"
    );
||||||| 9394ee7399
=======
use std::ffi::OsString;
use std::fs;
use std::process::Output;

use super::separate_build::two_standalone_roots;
use super::{command, own_units, workspace_below_the_root};
use crate::io::Repo;

fn toolchain_cargo(repo: &Repo) -> String {
    let printed = command("rustc")
        .current_dir(repo.root())
        .args(["--print", "sysroot"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(printed.status.success(), "{printed:?}");
    format!(
        "{}/bin/cargo",
        String::from_utf8_lossy(&printed.stdout).trim()
    )
}

fn install_driver(repo: &Repo) -> OsString {
    repo.write_unstaged(
        "driver/cargo.rs",
        r#"fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "metadata") {
        let required = std::env::var_os("REQUIRE_TOOLCHAIN").is_some();
        if required && !args[0].starts_with('+') {
            eprintln!("metadata probe without the selected toolchain");
            std::process::exit(9);
        }
        let rest: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('+')).collect();
        std::process::exit(std::process::Command::new("@CARGO@").args(rest).status().unwrap().code().unwrap_or(1));
    }
}
"#
        .replace("@CARGO@", &toolchain_cargo(repo))
        .as_str(),
    );
    let compiled = command("rustc")
        .current_dir(repo.root())
        .args(["--edition=2024", "driver/cargo.rs", "-o", "driver/cargo"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(compiled.status.success(), "{compiled:?}");
    fs::create_dir_all(repo.root().join("driver/path"))
        .unwrap_or_else(|error| unreachable!("{error}"));
    fs::copy(
        repo.root().join("driver/cargo"),
        repo.root().join("driver/path/cargo"),
    )
    .unwrap_or_else(|error| unreachable!("{error}"));
    std::env::join_paths(std::iter::once(repo.root().join("driver/path")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap_or_else(|error| unreachable!("{error}"))
}

fn run_from(repo: &Repo, directory: &str, path: &OsString, arguments: &[&str]) -> Output {
    let mut run = command(env!("CARGO_BIN_EXE_velnor-ci"));
    if arguments.contains(&"+nightly") {
        run.env("REQUIRE_TOOLCHAIN", "1");
    }
    run.current_dir(repo.root().join(directory))
        .env("PATH", path)
        .env("RUSTC_BOOTSTRAP", "1")
        .args(["inputs", "build", "--"])
        .args(arguments)
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn every_metadata_probe_runs_under_the_toolchain_the_command_selects() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    let path = install_driver(&repo);
    let driver = repo.root().join("driver/cargo");
    let driver = driver.to_str().unwrap_or_else(|| unreachable!());
    let built = run_from(
        &repo,
        "rust",
        &path,
        &[driver, "+nightly", "build", "--workspace"],
    );
    assert!(built.status.success(), "{built:?}");
}

#[test]
fn a_relative_cargo_executable_is_probed_from_the_directory_the_command_started_in() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    let path = install_driver(&repo);
    let built = run_from(
        &repo,
        "",
        &path,
        &[
            "./driver/cargo",
            "-Z",
            "unstable-options",
            "-C",
            "rust",
            "build",
            "--workspace",
        ],
    );
    assert!(built.status.success(), "{built:?}");
}

#[test]
fn a_relative_cargo_executable_is_probed_for_every_root_from_the_directory_the_command_started_in()
{
    let repo = two_standalone_roots();
    let path = install_driver(&repo);
    let built = run_from(&repo, "web", &path, &["../driver/cargo", "build"]);
    assert!(built.status.success(), "{built:?}");
}

#[test]
fn checkout_discovery_runs_the_cargo_executable_the_command_names() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    let _ = install_driver(&repo);
    repo.write_unstaged("wrong/cargo", "#!/bin/sh\necho wrong cargo >&2\nexit 7\n");
    let wrong = repo.root().join("wrong/cargo");
    fs::set_permissions(&wrong, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .unwrap_or_else(|error| unreachable!("{error}"));
    let path = std::env::join_paths(std::iter::once(repo.root().join("wrong")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap_or_else(|error| unreachable!("{error}"));
    let driver = repo.root().join("driver/cargo");
    let driver = driver.to_str().unwrap_or_else(|| unreachable!());
    let built = run_from(&repo, "rust", &path, &[driver, "build", "--workspace"]);
    assert!(built.status.success(), "{built:?}");
>>>>>>> origin/main
}
