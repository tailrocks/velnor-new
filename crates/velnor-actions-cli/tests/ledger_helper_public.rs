// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult = Result<(), Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_velnor-actions");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ledger_helper");
use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_REJECTION_MARKER, LEDGER_CLI_SMOKE_OP,
};

const EMIT_OP: &str = "emit-group-ledger-v1";
const ASSEMBLE_OP: &str = "assemble-group-ledgers-v1";
const MANIFEST_PATH: &str = ".github/velnor/task-execution-manifest-v1.json";
const RUN_DIR: &str = "velnor/r4242-a1";
const MATRIX_KEY: &str = "m-df42bdfcb20b0e49";
const REPORT_ID: &str = "task-r4242-a1-m-df42bdfcb20b0e49-4fcc45b68b3849cf";
const CLOCK_KEYS: [&str; 2] = ["started_at", "finished_at"];
const CLOCK_PLACEHOLDER: &str = "<clock>";
#[cfg(not(feature = "generator"))]
const THIN_REJECTION: &str = "velnor-actions: this ledger helper build only runs emit-group-ledger-v1 and assemble-group-ledgers-v1 through VELNOR_INTERNAL_OP\n";

struct Stage {
    root: tempfile::TempDir,
}

impl Stage {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let fixtures = Path::new(FIXTURES);
        copy_tree(
            &fixtures.join("emit/velnor"),
            &root.path().join("runner_temp/velnor"),
        )?;
        let manifest = root.path().join("checkout").join(MANIFEST_PATH);
        fs::create_dir_all(manifest.parent().ok_or("manifest parent")?)?;
        fs::copy(
            fixtures.join("emit/task-execution-manifest-v1.json"),
            manifest,
        )?;
        fs::copy(fixtures.join("event.json"), root.path().join("event.json"))?;
        Ok(Self { root })
    }

    fn runner_temp(&self) -> PathBuf {
        self.root.path().join("runner_temp")
    }

    fn run_dir(&self) -> PathBuf {
        self.runner_temp().join(RUN_DIR)
    }

    fn report(&self) -> PathBuf {
        self.run_dir()
            .join(MATRIX_KEY)
            .join("tasks")
            .join(format!("{REPORT_ID}.json"))
    }

    fn run(&self, op: &str, status: &str) -> Result<Output, Box<dyn Error>> {
        self.run_with(op, status, Some(self.runner_temp().as_os_str()))
    }

    fn run_with(
        &self,
        op: &str,
        status: &str,
        runner_temp: Option<&std::ffi::OsStr>,
    ) -> Result<Output, Box<dyn Error>> {
        let mut command = Command::new(BIN);
        command
            .env_clear()
            .current_dir(self.root.path().join("checkout"))
            .env("VELNOR_INTERNAL_OP", op)
            .env("VELNOR_JOB_STATUS", status)
            .env("VELNOR_MATRIX_KEY", MATRIX_KEY)
            .env("GITHUB_RUN_ID", "4242")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_JOB", "t54-group")
            .env("GITHUB_EVENT_NAME", "pull_request")
            .env("GITHUB_EVENT_PATH", self.root.path().join("event.json"))
            .env("GITHUB_SHA", "0123456789abcdef0123456789abcdef01234567")
            .env("GITHUB_REF", "refs/pull/1/merge")
            .env(
                "GITHUB_WORKFLOW_SHA",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            );
        if let Some(value) = runner_temp {
            command.env("RUNNER_TEMP", value);
        }
        Ok(command.output()?)
    }

    fn stdout_relative(&self, output: &Output) -> Result<String, Box<dyn Error>> {
        let text = String::from_utf8(output.stdout.clone())?;
        let path = PathBuf::from(text.trim_end_matches('\n'));
        Ok(path
            .strip_prefix(self.runner_temp())?
            .to_string_lossy()
            .into_owned())
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn golden(name: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        Path::new(FIXTURES).join("golden").join(name),
    )?)
}

fn clock_values(text: &str) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    for key in CLOCK_KEYS {
        let marker = format!("\"{key}\":\"");
        let mut rest = text;
        while let Some(at) = rest.find(&marker) {
            let after = &rest[at + marker.len()..];
            let end = after.find('"').unwrap_or(after.len());
            found.push((key, after[..end].to_owned()));
            rest = &after[end..];
        }
    }
    found
}

fn mask_clock(text: &str) -> String {
    let mut masked = text.to_owned();
    for (key, value) in clock_values(text) {
        masked = masked.replace(
            &format!("\"{key}\":\"{value}\""),
            &format!("\"{key}\":\"{CLOCK_PLACEHOLDER}\""),
        );
    }
    masked
}

fn rfc3339_seconds(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20 {
        return None;
    }
    for (at, byte) in bytes.iter().enumerate() {
        let ok = match at {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        };
        if !ok {
            return None;
        }
    }
    let part = |from: usize, to: usize| value[from..to].parse::<i64>().ok();
    let (year, month, day) = (part(0, 4)?, part(5, 7)?, part(8, 10)?);
    let (hour, minute, second) = (part(11, 13)?, part(14, 16)?, part(17, 19)?);
    let shifted_year = year - i64::from(month <= 2);
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let month_part = (month + 9) % 12;
    let day_of_year = (153 * month_part + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn wall_seconds() -> Result<i64, Box<dyn Error>> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?)
}

#[test]
fn emit_writes_the_golden_ledger_with_clock_fields_from_the_report_duration() -> TestResult {
    let stage = Stage::new()?;
    let before = wall_seconds()?;
    let output = stage.run(EMIT_OP, "success")?;
    let after = wall_seconds()?;
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let relative = stage.stdout_relative(&output)?;
    assert_eq!(
        relative,
        format!("{RUN_DIR}/{MATRIX_KEY}/group-ledger.json")
    );
    let text = fs::read_to_string(stage.runner_temp().join(&relative))?;
    let clocks = clock_values(&text);
    assert_eq!(clocks.len(), 2, "{clocks:?}");
    let started = rfc3339_seconds(&clocks[0].1).ok_or("started_at is not RFC3339")?;
    let finished = rfc3339_seconds(&clocks[1].1).ok_or("finished_at is not RFC3339")?;
    assert!(
        before <= finished && finished <= after,
        "{before} {finished} {after}"
    );
    assert_eq!(finished - started, 1, "duration_ms 1500 floors to 1 s");
    assert_eq!(mask_clock(&text), golden("emit_success.json")?);
    Ok(())
}

#[test]
fn emit_on_a_failed_job_without_a_report_records_needs_prepare() -> TestResult {
    let stage = Stage::new()?;
    fs::remove_file(stage.report())?;
    let output = stage.run(EMIT_OP, "failure")?;
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let relative = stage.stdout_relative(&output)?;
    let text = fs::read_to_string(stage.runner_temp().join(relative))?;
    let clocks = clock_values(&text);
    assert_eq!(clocks.len(), 2, "{clocks:?}");
    assert_eq!(clocks[0].1, clocks[1].1, "no report means a zero duration");
    assert_eq!(mask_clock(&text), golden("emit_needs_prepare.json")?);
    Ok(())
}

#[test]
fn emit_on_a_successful_job_without_a_report_fails_with_the_golden_stderr() -> TestResult {
    let stage = Stage::new()?;
    fs::remove_file(stage.report())?;
    let output = stage.run(EMIT_OP, "success")?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stderr)?,
        golden("emit_missing_report.stderr")?
    );
    assert!(
        !stage
            .run_dir()
            .join(MATRIX_KEY)
            .join("group-ledger.json")
            .exists()
    );
    Ok(())
}

#[test]
fn assemble_sorts_statically_staged_ledgers_by_job_key() -> TestResult {
    let stage = Stage::new()?;
    let reports = stage.run_dir().join("reports");
    copy_tree(&Path::new(FIXTURES).join("assemble/reports"), &reports)?;
    let output = stage.run(ASSEMBLE_OP, "success")?;
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        stage.stdout_relative(&output)?,
        format!("{RUN_DIR}/group-ledgers.json")
    );
    let assembled = fs::read_to_string(stage.run_dir().join("group-ledgers.json"))?;
    assert_eq!(assembled, golden("group_ledgers.json")?);
    Ok(())
}

#[test]
fn assemble_rejects_a_corrupt_staged_ledger_with_the_golden_stderr() -> TestResult {
    let stage = Stage::new()?;
    let bad = stage.run_dir().join("reports/bad-artifact");
    fs::create_dir_all(&bad)?;
    fs::write(bad.join("group-ledger.json"), b"not a ledger")?;
    let output = stage.run(ASSEMBLE_OP, "success")?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stderr)?,
        golden("assemble_corrupt.stderr")?
    );
    Ok(())
}

#[test]
fn a_ledger_op_without_an_absolute_runner_temp_does_nothing() -> TestResult {
    for runner_temp in [None, Some("relative/temp")] {
        let stage = Stage::new()?;
        let output = stage.run_with(EMIT_OP, "success", runner_temp.map(std::ffi::OsStr::new))?;
        assert_eq!(output.status.code(), Some(2), "{runner_temp:?} {output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stderr)?,
            expected_unrunnable_stderr(&stage)?,
            "{runner_temp:?}"
        );
        assert!(
            !stage
                .run_dir()
                .join(MATRIX_KEY)
                .join("group-ledger.json")
                .exists()
        );
    }
    Ok(())
}

#[cfg(feature = "generator")]
fn expected_unrunnable_stderr(stage: &Stage) -> Result<String, Box<dyn Error>> {
    let bare = Command::new(BIN)
        .env_clear()
        .current_dir(stage.root.path().join("checkout"))
        .output()?;
    assert_eq!(bare.status.code(), Some(2), "{bare:?}");
    Ok(String::from_utf8(bare.stderr)?)
}

#[cfg(not(feature = "generator"))]
fn expected_unrunnable_stderr(_stage: &Stage) -> Result<String, Box<dyn Error>> {
    Ok(THIN_REJECTION.to_owned())
}

#[cfg(not(feature = "generator"))]
#[test]
fn the_ledger_helper_build_rejects_every_other_internal_op() -> TestResult {
    let stage = Stage::new()?;
    for op in ["plan-v1", "merge-v1", "cache-publish-v1", "unknown-op"] {
        let output = stage.run(op, "success")?;
        assert_eq!(output.status.code(), Some(2), "{op}: {output:?}");
        assert!(output.stdout.is_empty(), "{op}: {output:?}");
        assert_eq!(String::from_utf8(output.stderr)?, THIN_REJECTION, "{op}");
    }
    Ok(())
}

#[test]
fn the_smoke_probe_tells_the_helper_build_from_the_full_build() -> TestResult {
    let stage = Stage::new()?;
    let output = Command::new(BIN)
        .env_clear()
        .env("VELNOR_INTERNAL_OP", LEDGER_CLI_SMOKE_OP)
        .current_dir(stage.root.path().join("checkout"))
        .output()?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(
        stderr.contains(LEDGER_CLI_REJECTION_MARKER),
        !cfg!(feature = "generator"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn the_ledger_helper_feature_compiles_only_the_contract_crate() -> TestResult {
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--locked",
            "--offline",
            "--package",
            "velnor-actions-cli",
            "--no-default-features",
            "--features",
            "ledger-helper",
            "--edges",
            "normal",
            "--prefix",
            "none",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let tree = String::from_utf8(output.stdout)?;
    let packages: Vec<&str> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(packages.contains(&"velnor-actions-contract"), "{tree}");
    for generator in [
        "velnor-actions-orchestrator",
        "velnor-actions-workflow-renderer",
        "velnor-actions-freshness",
        "velnor-actions-mise",
        "velnor-actions-tofu",
        "clap",
    ] {
        assert!(!packages.contains(&generator), "{generator} in {tree}");
    }
    Ok(())
}
