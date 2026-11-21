// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::scratch;
use crate::run_nextest::{archive_in, list_args, listed};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RECORD_RUN_DESTINATION: &str = "p=; for a in \"$@\"; do [ \"$p\" = --extract-to ] && printf '%s' \"$a\" > \"$RUNNER_TEMP/run-extract\"; case $a in --extract-to=*) printf '%s' \"${a#--extract-to=}\" > \"$RUNNER_TEMP/run-extract\";; esac; p=$a; done\n";
const WAIT_FOR_LIST: &str = "i=0\nwhile [ ! -f \"$RUNNER_TEMP/list-argv\" ] && [ $i -lt 40 ]; do sleep 0.05; i=$((i+1)); done\n";
const RUN_BODY: &str = "echo '    Starting 1 tests across 1 binary' >&2\n";
const RUN_END: &str = "echo '        PASS [   0.012s] (1/1) kept alpha' >&2\necho '     Summary [   0.012s] 1 test run: 1 passed, 0 skipped' >&2\n";
const WITHHELD: &[&str] = &["tests_listed=", "tests_ignored=", "tests_by_package="];

struct Scenario {
    list: String,
    wait: bool,
    tmpdir: Option<PathBuf>,
}

impl Scenario {
    fn listing(json: &str) -> Self {
        let list = format!(
            "printf '%s\\n' \"$@\" > \"$RUNNER_TEMP/list-argv\"\n\
p=; for a in \"$@\"; do d=; [ \"$p\" = --extract-to ] && d=$a; [ -n \"$d\" ] && [ -d \"$d\" ] && : > \"$RUNNER_TEMP/list-dest-existed\"; p=$a; done\n\
printf '%s\\n' '{json}'\nexit 0"
        );
        Self {
            list,
            wait: true,
            tmpdir: None,
        }
    }

    fn failing(list: &str) -> Self {
        Self {
            list: list.to_owned(),
            wait: false,
            tmpdir: None,
        }
    }

    fn execute(
        &self,
        name: &str,
        command: &[String],
        archive: Option<u64>,
    ) -> (Output, String, PathBuf) {
        let temp = scratch(name);
        archive_in(&temp);
        if let Some(size) = archive {
            let file =
                fs::File::create(temp.join("a.tar.zst")).unwrap_or_else(|e| unreachable!("{e}"));
            file.set_len(size).unwrap_or_else(|e| unreachable!("{e}"));
        }
        let program = temp.join("cargo-nextest");
        let wait = if self.wait { WAIT_FOR_LIST } else { "" };
        let script = format!(
            "#!/bin/sh\nif [ \"$2\" = list ]; then\n{}\nfi\n{RECORD_RUN_DESTINATION}{RUN_BODY}{wait}{RUN_END}",
            self.list
        );
        fs::write(&program, script).unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let outputs = temp.join("github-output");
        fs::write(&outputs, "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
        let path = format!(
            "{}:{}",
            temp.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        child
            .arg("run")
            .arg("--")
            .args(command)
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", &outputs)
            .env("PATH", path);
        if let Some(tmpdir) = &self.tmpdir {
            child.env("TMPDIR", tmpdir);
        }
        let output = child.output().unwrap_or_else(|e| unreachable!("{e}"));
        let written = fs::read_to_string(&outputs).unwrap_or_default();
        (output, written, temp)
    }
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

fn extract_to(args: &[String]) -> Option<String> {
    args.iter()
        .position(|arg| arg == "--extract-to")
        .and_then(|index| args.get(index + 1).cloned())
}

fn assert_withheld(written: &str) {
    assert!(written.contains("tests_run=1\n"), "{written}");
    for name in WITHHELD {
        assert!(!written.contains(name), "{name} leaked into {written}");
    }
}

fn assert_own_directory(temp: &Path, run_value: &str) {
    let args = list_args(temp);
    let value = extract_to(&args).unwrap_or_else(|| unreachable!("{args:?}"));
    assert!(
        args.iter().all(|arg| arg != "--extract-overwrite"),
        "{args:?}"
    );
    let recorded = fs::read_to_string(temp.join("run-extract")).unwrap_or_default();
    assert_eq!(recorded, run_value, "the run kept its own destination");
    assert_ne!(value, run_value, "{args:?}");
    let destination = Path::new(&value);
    assert!(
        destination.starts_with(temp),
        "{value} is not under {}",
        temp.display()
    );
    assert!(
        temp.join("list-dest-existed").exists(),
        "{value} did not exist when the list ran"
    );
    assert!(!destination.exists(), "{value} was left behind");
}

#[test]
fn the_early_list_extracts_the_archive_into_its_own_directory_for_every_run_spelling() {
    for (name, extract, run_value) in [
        ("tests-run-extract-spaced", &["--extract-to", "."][..], "."),
        ("tests-run-extract-equals", &["--extract-to=."][..], "."),
    ] {
        let one = listed(&[("kept", &["alpha"])]);
        let (output, written, temp) =
            Scenario::listing(&one).execute(name, &archive_run(extract), None);
        assert!(output.status.success(), "{output:?}");
        assert_own_directory(&temp, run_value);
        assert!(written.contains("tests_by_package=kept=1/1"), "{written}");
    }
}

#[test]
fn the_early_list_gets_a_directory_even_when_the_run_names_none() {
    let one = listed(&[("kept", &["alpha"])]);
    let (output, written, temp) =
        Scenario::listing(&one).execute("tests-run-extract-none", &archive_run(&[]), None);
    assert!(output.status.success(), "{output:?}");
    assert_own_directory(&temp, "");
    assert!(written.contains("tests_by_package=kept=1/1"), "{written}");
}

#[test]
fn a_run_that_extracts_nothing_lists_without_a_destination() {
    let one = listed(&[("kept", &["alpha"])]);
    let command = ["cargo-nextest", "nextest", "run", "--workspace"].map(str::to_owned);
    let (output, _, temp) =
        Scenario::listing(&one).execute("tests-run-extract-workspace", &command, None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(extract_to(&list_args(&temp)), None);
    assert!(!temp.join("list-dest-existed").exists());
}

#[test]
fn the_early_list_does_not_extract_into_an_unusable_tmpdir() {
    let mut scenario = Scenario::listing(&listed(&[("kept", &["alpha"])]));
    let probe = scratch("tests-run-extract-tmpdir-parent");
    let unusable = probe.join("unusable-tmpdir");
    fs::create_dir_all(&unusable).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&unusable, fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|e| unreachable!("{e}"));
    scenario.tmpdir = Some(unusable.clone());
    let (output, written, temp) = scenario.execute(
        "tests-run-extract-noexec-tmpdir",
        &archive_run(&["--extract-to", "."]),
        None,
    );
    fs::set_permissions(&unusable, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let value = extract_to(&list_args(&temp)).unwrap_or_default();
    assert!(Path::new(&value).starts_with(&temp), "{value}");
    assert!(written.contains("tests_by_package="), "{written}");
}

#[test]
fn a_list_that_cannot_execute_is_reported_and_withholds_every_list_count() {
    let scenario = Scenario::failing(
        "echo 'Error: [double-spawn] failed: Permission denied (os error 13)' >&2\nexit 104",
    );
    let (output, written, _) = scenario.execute(
        "tests-run-extract-denied",
        &archive_run(&["--extract-to", "."]),
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert_withheld(&written);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Permission denied"), "{stderr}");
    assert!(
        stderr.contains("velnor-ci: the early nextest list failed"),
        "{stderr}"
    );
    assert!(stderr.contains("exit code 104"), "{stderr}");
}

#[test]
fn a_list_that_prints_no_usable_listing_is_reported_and_withholds_every_list_count() {
    let (output, written, _) = Scenario::listing("not json").execute(
        "tests-run-extract-unparsable",
        &archive_run(&["--extract-to", "."]),
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert_withheld(&written);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("velnor-ci: the early nextest list failed: it printed no usable listing"),
        "{stderr}"
    );
}

#[test]
fn a_failed_list_without_an_archive_withholds_the_counts_too() {
    let command = ["cargo-nextest", "nextest", "run", "--workspace"].map(str::to_owned);
    let (output, written, _) =
        Scenario::failing("exit 3").execute("tests-run-extract-workspace-fails", &command, None);
    assert!(output.status.success(), "{output:?}");
    assert_withheld(&written);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("velnor-ci: the early nextest list failed"),
        "{stderr}"
    );
}

#[test]
fn an_extraction_larger_than_the_free_space_is_refused_before_the_list_runs() {
    let one = listed(&[("kept", &["alpha"])]);
    let (output, written, temp) = Scenario::listing(&one).execute(
        "tests-run-extract-no-room",
        &archive_run(&["--extract-to", "."]),
        Some(1 << 42),
    );
    assert!(output.status.success(), "{output:?}");
    assert_withheld(&written);
    assert!(!temp.join("list-argv").exists(), "the list ran anyway");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("velnor-ci: the early nextest list failed"),
        "{stderr}"
    );
    assert!(stderr.contains("bytes free"), "{stderr}");
    let leftovers = fs::read_dir(&temp)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("velnor-nextest-list")
        })
        .count();
    assert_eq!(leftovers, 0);
}
