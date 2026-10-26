// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use velnor_actions_orchestrator::{assemble_merge_request, retrieve_reports, write_task_report};

use super::TestResult;

const CHILD_CASE_ENV: &str = "VELNOR_TASK_REPORT_CHILD_CASE";
const CHILD_OPERATION_ENV: &str = "VELNOR_TASK_REPORT_CHILD_OPERATION";
const CHILD_RESULT_ENV: &str = "VELNOR_TASK_REPORT_CHILD_RESULT";
const CHILD_RUN_KEY_ENV: &str = "VELNOR_TASK_REPORT_RUN_KEY";
const CHILD_RUN_DIR_ENV: &str = "VELNOR_TASK_REPORT_RUN_DIR";
const CHILD_REQUEST_ENV: &str = "VELNOR_TASK_REPORT_REQUEST";
#[cfg(unix)]
const PROCESS_SNAPSHOT_MAX_BYTES: usize = 1_048_576;
#[cfg(unix)]
const PROCESS_SNAPSHOT_READ_LIMIT: u64 = 1_048_577;

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
        "retrieve" => {
            let count = retrieve_reports()?;
            let result = env::var_os(CHILD_RESULT_ENV)
                .ok_or_else(|| io::Error::other("retrieval result path missing"))?;
            fs::write(result, count.to_string())?;
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
    let process_group = operation == "retrieve";
    #[cfg(unix)]
    if process_group {
        command.process_group(0);
    }
    for (name, value) in variables {
        command.env(name, value);
    }
    let child = command.spawn()?;
    let mut guard = ChildGuard::new(child, process_group);
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
                return Err(child_error("child timed out", &stdout_path, &stderr_path).into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                guard.reap();
                return Err(child_error(
                    &format!("child wait failed: {error}"),
                    &stdout_path,
                    &stderr_path,
                )
                .into());
            }
        }
    };
    if !status.success() {
        return Err(child_error(
            &format!("child exited {status}"),
            &stdout_path,
            &stderr_path,
        )
        .into());
    }
    Ok(())
}

fn child_error(message: &str, stdout_path: &Path, stderr_path: &Path) -> io::Error {
    io::Error::other(format!(
        "{message}; stdout={}; stderr={}",
        bounded_log(stdout_path),
        bounded_log(stderr_path)
    ))
}

struct ChildGuard {
    child: Option<Child>,
    process_group: bool,
}

impl ChildGuard {
    fn new(child: Child, process_group: bool) -> Self {
        Self {
            child: Some(child),
            process_group,
        }
    }

    fn reap(&mut self) {
        if let Some(mut child) = self.child.take()
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            if self.process_group {
                #[cfg(unix)]
                terminate_process_tree(&child);
                #[cfg(not(unix))]
                let _kill_result = child.kill();
            } else {
                let _kill_result = child.kill();
            }
            let _wait_result = child.wait();
        }
    }
}

#[cfg(unix)]
fn terminate_process_tree(child: &Child) {
    let root_group = rustix::process::Pid::from_child(child).as_raw_pid();
    let parent_group = rustix::process::getpgrp().as_raw_pid();
    signal_process_group(root_group, rustix::process::Signal::STOP);

    let first = descendant_process_groups(root_group, parent_group).unwrap_or_default();
    for group in &first {
        signal_process_group(*group, rustix::process::Signal::STOP);
    }
    let final_groups = descendant_process_groups(root_group, parent_group).unwrap_or_default();
    for group in first.union(&final_groups) {
        signal_process_group(*group, rustix::process::Signal::KILL);
    }
    signal_process_group(root_group, rustix::process::Signal::KILL);
}

#[cfg(unix)]
fn descendant_process_groups(root_group: i32, parent_group: i32) -> io::Result<HashSet<i32>> {
    let mut rows = Vec::new();
    let snapshot = process_snapshot()?;
    for line in String::from_utf8_lossy(&snapshot).lines() {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|field| field.parse::<i32>().ok()) else {
            continue;
        };
        let Some(parent) = fields.next().and_then(|field| field.parse::<i32>().ok()) else {
            continue;
        };
        let Some(group) = fields.next().and_then(|field| field.parse::<i32>().ok()) else {
            continue;
        };
        if pid > 1 && parent >= 0 && group > 1 {
            rows.push((pid, parent, group));
        }
    }

    let mut children = HashMap::new();
    for (pid, parent, _) in &rows {
        children.entry(*parent).or_insert_with(Vec::new).push(*pid);
    }
    let mut descendants = HashSet::new();
    let mut pending = VecDeque::from([root_group]);
    while let Some(parent) = pending.pop_front() {
        if let Some(processes) = children.get(&parent) {
            for pid in processes {
                if descendants.insert(*pid) {
                    pending.push_back(*pid);
                }
            }
        }
    }

    Ok(rows
        .iter()
        .filter(|(pid, _, group)| {
            descendants.contains(pid) && *group != root_group && *group != parent_group
        })
        .map(|(_, _, group)| *group)
        .collect())
}

#[cfg(unix)]
fn process_snapshot() -> io::Result<Vec<u8>> {
    let mut child = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid="])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let Some(stdout) = child.stdout.take() else {
        let _kill_result = child.kill();
        let _wait_result = child.wait();
        return Err(io::Error::other("process snapshot stdout unavailable"));
    };
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(PROCESS_SNAPSHOT_READ_LIMIT)
            .read_to_end(&mut bytes);
        (bytes, result)
    });
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (bytes, result) = reader
                    .join()
                    .map_err(|_| io::Error::other("process snapshot reader failed"))?;
                result?;
                if bytes.len() > PROCESS_SNAPSHOT_MAX_BYTES {
                    return Err(io::Error::other("process snapshot exceeded limit"));
                }
                if !status.success() {
                    return Err(io::Error::other("process snapshot failed"));
                }
                return Ok(bytes);
            }
            Ok(None) if Instant::now() >= deadline => {
                let _kill_result = child.kill();
                let _wait_result = child.wait();
                let _reader_result = reader.join();
                return Err(io::Error::other("process snapshot timed out"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                let _kill_result = child.kill();
                let _wait_result = child.wait();
                let _reader_result = reader.join();
                return Err(error);
            }
        }
    }
}

#[cfg(unix)]
fn signal_process_group(group: i32, signal: rustix::process::Signal) {
    if let Some(group) = rustix::process::Pid::from_raw(group) {
        let _signal_result = rustix::process::kill_process_group(group, signal);
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
