// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, command, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";

fn prepared() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../../shared\n");
    repo.set_mtime(OUTPUT, UNIX_EPOCH + Duration::from_mins(50_000_000));
    repo.write_unstaged("driver/cargo.rs", r#"use std::fs::{self, File, FileTimes};
use std::time::{Duration, UNIX_EPOCH};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "metadata") {
        std::process::exit(std::process::Command::new("cargo").args(&args).status().unwrap().code().unwrap_or(1));
    }
    let marker = fs::read_dir("target/.velnor-input-inventories").unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with(".build-clock-"))
        .unwrap();
    let marker = fs::metadata(marker).unwrap().modified().unwrap();
    let mode = std::env::var("CLOCK_CASE").unwrap();
    let modified = match mode.as_str() {
        "restored" => UNIX_EPOCH + Duration::from_secs(3_000_000_000),
        "older" => marker - Duration::from_secs(1),
        "unseen" => {
            let stamp = std::path::Path::new("target/cache-stamp");
            if !stamp.exists() {
                let age = marker.duration_since(UNIX_EPOCH).unwrap();
                fs::write(stamp, format!("{} {}", age.as_secs(), age.subsec_nanos())).unwrap();
            }
            let text = fs::read_to_string(stamp).unwrap();
            let (seconds, nanos) = text.split_once(' ').unwrap();
            UNIX_EPOCH + Duration::new(seconds.parse().unwrap(), nanos.parse().unwrap())
        }
        _ => marker,
    };
    let output = "target/debug/build/lib-f000000000000008/output";
    let text = if mode == "restored" {
        "cargo:rerun-if-changed=../../shared\n"
    } else {
        "cargo:rerun-if-changed=../../shared\ncargo:rustc-env=CLOCK_OUTPUT=changed\n"
    };
    fs::write(output, text).unwrap();
    File::options().write(true).open(output).unwrap().set_times(FileTimes::new().set_modified(modified)).unwrap();
}
"#);
    let compiled = command("rustc")
        .current_dir(repo.root())
        .args(["--edition=2024", "driver/cargo.rs", "-o", "driver/cargo"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    assert!(compiled.status.success(), "{compiled:?}");
    repo
}

fn build(repo: &Repo, mode: &str) -> Output {
    command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .env("CLOCK_CASE", mode)
        .args(["inputs", "build", "--"])
        .arg(repo.root().join("driver/cargo"))
        .args(["build", "--workspace"])
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

fn stale(repo: &Repo, built: &Output) {
    assert_eq!(built.status.code(), Some(1), "{built:?}");
    assert!(
        !repo
            .root()
            .join(format!("{OUTPUT}.velnor-inputs.json"))
            .exists()
    );
    let refused = audit(repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_changed_build_output_at_the_filesystem_clock_is_fresh() {
    let repo = prepared();
    let built = build(&repo, "equal");
    assert!(built.status.success(), "{built:?}");
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|error| unreachable!("{error}")),
        "shared\tlib\n"
    );
}

#[test]
fn restoring_the_prebuild_output_cannot_attach_a_new_inventory() {
    let repo = prepared();
    stale(&repo, &build(&repo, "restored"));
}

#[test]
fn a_changed_build_output_older_than_the_filesystem_clock_is_stale() {
    let repo = prepared();
    stale(&repo, &build(&repo, "older"));
}

#[test]
fn an_unseen_cached_output_at_the_filesystem_clock_remains_stale() {
    let repo = prepared();
    fs::remove_file(repo.root().join(OUTPUT)).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(
        !repo
            .root()
            .join(format!("{OUTPUT}.velnor-inputs.json"))
            .exists()
    );
    stale(&repo, &build(&repo, "unseen"));
}
