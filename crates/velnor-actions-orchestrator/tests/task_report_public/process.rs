// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use velnor_actions_orchestrator::{assemble_merge_request, write_task_report};

use super::TestResult;

const CHILD_CASE_ENV: &str = "VELNOR_TASK_REPORT_CHILD_CASE";
const CHILD_OPERATION_ENV: &str = "VELNOR_TASK_REPORT_CHILD_OPERATION";
const CHILD_RESULT_ENV: &str = "VELNOR_TASK_REPORT_CHILD_RESULT";
const CHILD_RUN_KEY_ENV: &str = "VELNOR_TASK_REPORT_RUN_KEY";
const CHILD_RUN_DIR_ENV: &str = "VELNOR_TASK_REPORT_RUN_DIR";
const CHILD_REQUEST_ENV: &str = "VELNOR_TASK_REPORT_REQUEST";

pub(super) fn child_mode(test_name: &str) -> TestResult<bool> {
    let Ok(selected) = env::var(CHILD_CASE_ENV) else {
        return Ok(false);
    };
    assert_eq!(selected, test_name);
    match env::var(CHILD_OPERATION_ENV)?.as_str() {
        "write" => {
            let count = write_task_report()?;
            let result = env::var_os(CHILD_RESULT_ENV)
                .ok_or_else(|| io::Error::other("writer result path missing"))?;
            fs::write(result, count.to_string())?;
        }
        "assemble" => {
            let run_key = env::var(CHILD_RUN_KEY_ENV)?;
            let run_dir = env::var_os(CHILD_RUN_DIR_ENV)
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::other("assembler run directory missing"))?;
            let request = assemble_merge_request(&run_key, &run_dir)?;
            let output = env::var_os(CHILD_REQUEST_ENV)
                .ok_or_else(|| io::Error::other("assembler request path missing"))?;
            fs::write(output, request)?;
        }
        operation => {
            return Err(io::Error::other(format!("unknown child operation: {operation}")).into());
        }
    }
    Ok(true)
}

pub(super) fn run_child(
    test_name: &str,
    operation: &str,
    label: &str,
    root: &Path,
    variables: &[(OsString, OsString)],
) -> TestResult<()> {
    if !root.is_absolute() {
        return Err(io::Error::other("child working directory is not absolute").into());
    }
    let stdout_path = root.join(format!("{label}.stdout"));
    let stderr_path = root.join(format!("{label}.stderr"));
    let stdout = File::create(&stdout_path)?;
    let stderr = File::create(&stderr_path)?;
    let executable = env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .args(["--exact", test_name, "--nocapture"])
        .current_dir(root)
        .env_clear()
        .env(CHILD_CASE_ENV, test_name)
        .env(CHILD_OPERATION_ENV, operation)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    for (name, value) in variables {
        command.env(name, value);
    }
    let child = command.spawn()?;
    let mut guard = ChildGuard::new(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        let child = guard
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("child process state missing"))?;
        match child.try_wait() {
            Ok(Some(status)) => {
                guard.child.take();
                break status;
            }
            Ok(None) if Instant::now() >= deadline => {
                guard.reap();
                return Err(io::Error::other(format!(
                    "child timed out; stdout={}; stderr={}",
                    bounded_log(&stdout_path),
                    bounded_log(&stderr_path)
                ))
                .into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                guard.reap();
                return Err(io::Error::other(format!(
                    "child wait failed: {error}; stdout={}; stderr={}",
                    bounded_log(&stdout_path),
                    bounded_log(&stderr_path)
                ))
                .into());
            }
        }
    };
    if !status.success() {
        return Err(io::Error::other(format!(
            "child exited {status}; stdout={}; stderr={}",
            bounded_log(&stdout_path),
            bounded_log(&stderr_path)
        ))
        .into());
    }
    Ok(())
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn reap(&mut self) {
        if let Some(mut child) = self.child.take()
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            let _kill_result = child.kill();
            let _wait_result = child.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.reap();
    }
}

fn bounded_log(path: &Path) -> String {
    let Ok(file) = File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if file.take(8192).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
