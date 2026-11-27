// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use rustix::process::{Pid, Signal, kill_process};
use uuid::Uuid;

use super::contexts::variable;
use crate::job_message::AgentJobRequestMessage;

const TRACKING_VARIABLE: &str = "RUNNER_TRACKING_ID";
const CLEANUP_SWITCH: &str = "process.clean";

#[derive(Debug, Clone)]
pub(super) struct Tracking {
    id: String,
}

impl Tracking {
    pub(super) fn for_job(message: &AgentJobRequestMessage) -> Option<Self> {
        let enabled = match variable(message, CLEANUP_SWITCH).map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("false") => false,
            Some(_) | None => true,
        };
        enabled.then(|| Self {
            id: format!("github_{}", Uuid::new_v4()),
        })
    }

    pub(super) fn pair(&self) -> (String, String) {
        (TRACKING_VARIABLE.to_owned(), self.id.clone())
    }

    pub(super) fn reap(&self) {
        let marker = format!("{TRACKING_VARIABLE}={}", self.id);
        for pid in carrying(&marker) {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

fn pid_of(raw: &str) -> Option<Pid> {
    Pid::from_raw(raw.parse().ok()?)
}

#[cfg(target_os = "linux")]
fn carrying(marker: &str) -> Vec<Pid> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let pid = pid_of(name.to_str()?)?;
            let environment = std::fs::read(entry.path().join("environ")).ok()?;
            environment
                .split(|byte| *byte == 0)
                .any(|entry| entry == marker.as_bytes())
                .then_some(pid)
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn carrying(marker: &str) -> Vec<Pid> {
    let Ok(listing) = std::process::Command::new("ps")
        .args(["-axww", "-E", "-o", "pid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim_start().split_once(' ')?;
            if command.split(' ').any(|word| word == marker) {
                pid_of(pid)
            } else {
                None
            }
        })
        .collect()
}
