// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::scratch;
use crate::run_nextest::{execute_nextest, fake_nextest, list_args, listed};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const RUN_EXTRACT: &str = "p=; for a in \"$@\"; do [ \"$p\" = --extract-to ] && printf '%s' \"$a\" > \"$RUNNER_TEMP/run-extract\"; case $a in --extract-to=*) printf '%s' \"${a#--extract-to=}\" > \"$RUNNER_TEMP/run-extract\";; esac; p=$a; done\n\
echo '    Starting 1 tests across 1 binary' >&2\n\
i=0\n\
while [ ! -f \"$RUNNER_TEMP/list-argv\" ] && [ $i -lt 40 ]; do sleep 0.05; i=$((i+1)); done\n\
echo '        PASS [   0.012s] (1/1) kept alpha' >&2\n\
echo '     Summary [   0.012s] 1 test run: 1 passed, 0 skipped' >&2";

fn extract_to(args: &[String]) -> Option<String> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == "--extract-to" {
            return args.get(index + 1).cloned();
        }
        arg.strip_prefix("--extract-to=").map(ToOwned::to_owned)
    })
}

fn archive_run(extract: &[&str]) -> Vec<String> {
    let mut command = vec![
        "cargo-nextest".to_owned(),
        "nextest".to_owned(),
        "run".to_owned(),
        "--archive-file".to_owned(),
        "a.tar.zst".to_owned(),
    ];
    command.extend(extract.iter().map(|word| (*word).to_owned()));
    command.extend(
        ["--extract-overwrite", "--no-tests=pass"]
            .iter()
            .map(|word| (*word).to_owned()),
    );
    command
}

fn run(name: &str, command: &[String]) -> (std::process::Output, std::path::PathBuf) {
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    let one = listed(&[("kept", &["alpha"])]);
    let (output, _, temp, _) = execute_nextest(name, RUN_EXTRACT, &one, &words, None);
    (output, temp)
}

fn assert_separate_destination(temp: &Path, run_value: &str) {
    let args = list_args(temp);
    let list_value = extract_to(&args).unwrap_or_else(|| unreachable!("{args:?}"));
    let recorded = fs::read_to_string(temp.join("run-extract")).unwrap_or_default();
    assert_eq!(recorded, run_value, "the run kept its own destination");
    assert_ne!(list_value, run_value, "{args:?}");
    let destination = Path::new(&list_value);
    assert!(
        destination.starts_with(temp),
        "{list_value} is not under {}",
        temp.display()
    );
    assert!(
        temp.join("list-dest-existed").exists(),
        "{list_value} did not exist when the list ran"
    );
    assert!(!destination.exists(), "{list_value} was left behind");
}

#[test]
fn the_early_list_extracts_the_archive_into_its_own_directory() {
    let (output, temp) = run(
        "tests-run-extract-separate",
        &archive_run(&["--extract-to", "."]),
    );
    assert!(output.status.success(), "{output:?}");
    assert_separate_destination(&temp, ".");
}

#[test]
fn the_early_list_replaces_an_extract_to_written_with_an_equals_sign() {
    let (output, temp) = run(
        "tests-run-extract-equals",
        &archive_run(&["--extract-to=."]),
    );
    assert!(output.status.success(), "{output:?}");
    assert_separate_destination(&temp, ".");
    let args = list_args(&temp);
    assert!(
        args.iter().all(|arg| arg != "--extract-to"),
        "the equals form must stay one argument: {args:?}"
    );
}

#[test]
fn the_early_list_leaves_an_archive_without_extract_to_to_its_own_temporary_directory() {
    let (output, temp) = run("tests-run-extract-none", &archive_run(&[]));
    assert!(output.status.success(), "{output:?}");
    let args = list_args(&temp);
    assert_eq!(extract_to(&args), None, "{args:?}");
    assert!(!temp.join("list-dest-existed").exists());
}

#[test]
fn a_list_that_prints_no_listing_says_so() {
    let command = archive_run(&["--extract-to", "."]);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    let (output, written, _, _) = execute_nextest(
        "tests-run-extract-unparsable",
        RUN_EXTRACT,
        "not json",
        &words,
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!written.contains("tests_by_package="), "{written}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("velnor-ci: the early nextest list printed no usable listing"),
        "{stderr}"
    );
}

#[test]
fn the_early_list_does_not_extract_into_an_unusable_tmpdir() {
    let temp = scratch("tests-run-extract-noexec-tmpdir");
    fake_nextest(&temp, RUN_EXTRACT, &listed(&[("kept", &["alpha"])]));
    let unusable = temp.join("unusable-tmpdir");
    fs::create_dir_all(&unusable).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&unusable, fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = temp.join("github-output");
    fs::write(&outputs, "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(archive_run(&["--extract-to", "."]))
        .env("RUNNER_TEMP", &temp)
        .env("TMPDIR", &unusable)
        .env("GITHUB_OUTPUT", &outputs)
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&unusable, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let args = list_args(&temp);
    let list_value = extract_to(&args).unwrap_or_else(|| unreachable!("{args:?}"));
    assert!(
        Path::new(&list_value).starts_with(&temp) && !Path::new(&list_value).starts_with(&unusable),
        "{list_value} must be under RUNNER_TEMP, not TMPDIR"
    );
    assert!(temp.join("list-dest-existed").exists());
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    assert!(written.contains("tests_by_package="), "{written}");
}
