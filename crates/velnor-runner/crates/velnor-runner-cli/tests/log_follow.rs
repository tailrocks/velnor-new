//! Follows the service log through the CLI.

use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn signal_process(pid: u32, signal: &str) {
    let process = pid.to_string();
    let _signal_result = Command::new("/bin/kill")
        .args([signal, process.as_str()])
        .status();
}

fn signal_group(pid: u32, signal: &str) {
    let group = format!("-{pid}");
    let _signal_result = Command::new("/bin/kill")
        .args([signal, group.as_str()])
        .status();
}

fn children_of(pid: u32) -> Result<Vec<u32>, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("process list failed".to_owned());
    }
    let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let child = fields.next()?.parse::<u32>().ok()?;
            let parent = fields.next()?.parse::<u32>().ok()?;
            (parent == pid).then_some(child)
        })
        .collect())
}

fn process_exists(pid: u32) -> bool {
    let process = pid.to_string();
    Command::new("/bin/kill")
        .args(["-0", process.as_str()])
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn logs_follow_streams_new_service_output() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("velnor-cli-log-follow-{}", std::process::id()));
    let logs = root.join("Library/Logs/Velnor");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let path = logs.join("host.log");
    std::fs::write(&path, "first\n").map_err(|error| error.to_string())?;

    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-host"));
    command
        .env("HOME", &root)
        .args(["logs", "--follow"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let pid = child.id();
    let stdout = child.stdout.take().ok_or("missing log output")?;
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { return };
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    let result = (|| {
        let first = receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let mut log = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        log.write_all(b"second\n")
            .map_err(|error| error.to_string())?;
        let second = receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        if first != "first" || second != "second" {
            return Err(format!("unexpected log lines: {first:?}, {second:?}"));
        }
        if !children_of(pid)?.is_empty() {
            return Err("CLI left a log follower child".to_owned());
        }
        Ok(())
    })();

    signal_process(pid, "-TERM");
    let _status = child.wait().map_err(|error| error.to_string())?;
    signal_group(pid, "-KILL");
    drop(receiver);
    reader
        .join()
        .map_err(|_| "log reader did not stop".to_owned())?;
    let _removed = std::fs::remove_dir_all(&root);
    if process_exists(pid) {
        return Err("log follower remained after CLI termination".to_owned());
    }
    result
}
