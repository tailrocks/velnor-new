// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
        writeln!(log, "{} @ {}", args.join(" "), std::env::current_dir().unwrap().display()).unwrap();
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
    fs::create_dir_all(repo.root().join("driver/path"))
        .unwrap_or_else(|error| unreachable!("{error}"));
    fs::copy(
        repo.root().join("driver/cargo"),
        repo.root().join("driver/path/cargo"),
    )
    .unwrap_or_else(|error| unreachable!("{error}"));
    let path = std::env::join_paths(std::iter::once(repo.root().join("driver/path")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
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
    let selected = probes
        .lines()
        .filter(|probe| probe.starts_with("+nightly metadata "))
        .count();
    let plain = probes.lines().count() - selected;
    assert!(selected >= 2, "{probes} {built:?}");
    assert!(
        plain <= 1,
        "only the probe that reads the repository facts may ignore the selected toolchain: {probes}"
    );
}
