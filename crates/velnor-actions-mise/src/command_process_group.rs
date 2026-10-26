// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    os::unix::process::CommandExt,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process_group, test_kill_process_group, waitid,
};

use crate::ProcessGroupDrainReceipt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ObservedStatus {
    pub(super) code: Option<i32>,
    pub(super) signal: Option<i32>,
    pub(super) success: bool,
}

pub(super) struct ProcessGroupAnchor {
    child: Child,
    stdin: Option<ChildStdin>,
    pid: Pid,
}

impl ProcessGroupAnchor {
    pub(super) fn spawn() -> Result<Self, String> {
        let mut child = anchor_command("/bin/cat")
            .spawn()
            .map_err(|error| error.to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "anchor_stdin_missing".to_owned())?;
        let pid = child_pid(&child)?;
        if observe_exit(&child).is_err() {
            drop(stdin);
            return Err("anchor_waitability_unproven".to_owned());
        }
        Ok(Self {
            child,
            stdin: Some(stdin),
            pid,
        })
    }

    pub(super) fn pgid(&self) -> i32 {
        self.pid.as_raw_pid()
    }

    pub(super) fn drain(
        mut self,
        target: Option<&mut Child>,
        deadline: Instant,
    ) -> Result<ProcessGroupDrainReceipt, String> {
        let Ok(status) = observe_exit(&self.child) else {
            self.stdin.take();
            reap_best_effort(target, deadline);
            reap_best_effort(Some(&mut self.child), deadline);
            return Err("anchor_wait_ownership_lost".to_owned());
        };
        let anchor_terminal = status.is_some();
        let signal_result = kill_process_group(self.pid, Signal::KILL);
        self.stdin.take();
        if anchor_terminal {
            reap_bounded(target, &mut self.child, deadline);
            drop(wait_group_absent(self.pid, deadline));
            return Err("anchor_not_live".to_owned());
        }
        if let Err(error) = signal_result {
            reap_best_effort(target, deadline);
            reap_best_effort(Some(&mut self.child), deadline);
            return Err(if error == rustix::io::Errno::SRCH {
                "anchor_group_missing".to_owned()
            } else {
                format!("kill_group:{error}")
            });
        }
        let target_result = target.map(|child| reap_until(child, deadline));
        let anchor_result = reap_until(&mut self.child, deadline);
        let gone = wait_group_absent(self.pid, deadline);
        finish_drain(target_result, anchor_result, gone)?;
        Ok(ProcessGroupDrainReceipt::new())
    }
}

fn finish_drain(
    target_result: Option<Result<ExitStatus, String>>,
    anchor_result: Result<ExitStatus, String>,
    gone: Result<(), String>,
) -> Result<(), String> {
    target_result
        .transpose()
        .map_err(|error| format!("target_reap:{error}"))?;
    anchor_result.map_err(|error| format!("anchor_reap:{error}"))?;
    gone
}

fn reap_bounded(target: Option<&mut Child>, anchor: &mut Child, deadline: Instant) {
    if let Some(target) = target {
        drop(reap_until(target, deadline));
    }
    drop(reap_until(anchor, deadline));
}

fn reap_best_effort(target: Option<&mut Child>, deadline: Instant) {
    if let Some(target) = target {
        reap_best_effort_child(target, deadline);
    }
}

fn reap_best_effort_child(child: &mut Child, deadline: Instant) {
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= deadline => return,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn anchor_command(executable: &str) -> Command {
    let mut command = Command::new(executable);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear()
        .process_group(0);
    command
}

pub(super) fn observe_exit(child: &Child) -> Result<Option<ObservedStatus>, String> {
    let pid = child_pid(child)?;
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    let Some(status) = waitid(WaitId::Pid(pid), options).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    if !(status.exited() || status.killed() || status.dumped()) {
        return Ok(None);
    }
    let code = status.exit_status();
    Ok(Some(ObservedStatus {
        code,
        signal: status.terminating_signal(),
        success: code == Some(0),
    }))
}

pub(super) fn reap_observed(child: &mut Child) -> Result<ExitStatus, String> {
    child.wait().map_err(|error| error.to_string())
}

pub(super) fn reap_until(child: &mut Child, deadline: Instant) -> Result<ExitStatus, String> {
    loop {
        if observe_exit(child)?.is_some() {
            return child.wait().map_err(|error| error.to_string());
        }
        if Instant::now() >= deadline {
            return Err("reap_deadline_exhausted".to_owned());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn terminate_unanchored(child: &mut Child, deadline: Instant) -> Result<(), String> {
    if observe_exit(child).is_err() {
        return Err("child_status_unavailable".to_owned());
    }
    let pid = child_pid(child)?;
    match kill_process_group(pid, Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => {}
        Err(error) => return Err(format!("kill_group:{error}")),
    }
    reap_until(child, deadline).map_err(|error| format!("reap_child:{error}"))?;
    wait_group_absent(pid, deadline)
}

fn wait_group_absent(pid: Pid, deadline: Instant) -> Result<(), String> {
    loop {
        match test_kill_process_group(pid) {
            Ok(()) => {}
            Err(rustix::io::Errno::SRCH) => return Ok(()),
            Err(error) => return Err(format!("observe_group:{error}")),
        }
        if Instant::now() >= deadline {
            return Err("group_drain_deadline_exhausted".to_owned());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn child_pid(child: &Child) -> Result<Pid, String> {
    let raw = i32::try_from(child.id()).map_err(|_| "child_pid_overflow".to_owned())?;
    Pid::from_raw(raw).ok_or_else(|| "child_pid_invalid".to_owned())
}
