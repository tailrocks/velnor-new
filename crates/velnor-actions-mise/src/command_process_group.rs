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
        let mut child = anchor_command("/bin/sh")
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
        .args(["-c", "IFS= read -r _; kill -9 -$$"])
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
    wait_for_group_absence(
        || test_kill_process_group(pid),
        || Instant::now() >= deadline,
        || thread::sleep(Duration::from_millis(5)),
    )
}

fn wait_for_group_absence(
    mut observe: impl FnMut() -> Result<(), rustix::io::Errno>,
    mut expired: impl FnMut() -> bool,
    mut pause: impl FnMut(),
) -> Result<(), String> {
    loop {
        match observe() {
            Ok(()) | Err(rustix::io::Errno::PERM) => {}
            Err(rustix::io::Errno::SRCH) => return Ok(()),
            Err(error) => return Err(format!("observe_group:{error}")),
        }
        if expired() {
            return Err("group_drain_deadline_exhausted".to_owned());
        }
        pause();
    }
}

fn child_pid(child: &Child) -> Result<Pid, String> {
    let raw = i32::try_from(child.id()).map_err(|_| "child_pid_overflow".to_owned())?;
    Pid::from_raw(raw).ok_or_else(|| "child_pid_invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::wait_for_group_absence;
    use rustix::io::Errno;
    use std::cell::Cell;

    #[test]
    fn transient_indeterminate_group_probe_requires_confirmed_absence() {
        let polls = Cell::new(0);
        let ticks = Cell::new(0);
        let confirmed_absent = Cell::new(false);
        let result = wait_for_group_absence(
            || {
                let count = polls.get();
                polls.set(count + 1);
                match count {
                    0 | 1 => Err(Errno::PERM),
                    _ => {
                        confirmed_absent.set(true);
                        Err(Errno::SRCH)
                    }
                }
            },
            || ticks.get() >= 3,
            || ticks.set(ticks.get() + 1),
        );
        assert!(result.is_ok(), "only ESRCH establishes absence: {result:?}");
        assert!(
            confirmed_absent.get(),
            "success requires external absence evidence"
        );
    }

    #[test]
    fn persistent_indeterminate_group_probe_exhausts_the_existing_deadline() {
        let ticks = Cell::new(0);
        let result = wait_for_group_absence(
            || Err(Errno::PERM),
            || ticks.get() >= 2,
            || ticks.set(ticks.get() + 1),
        );
        assert_eq!(result, Err("group_drain_deadline_exhausted".to_owned()));
        assert!(
            ticks.get() >= 2,
            "indeterminate state must wait for the deadline"
        );
    }
    #[test]
    fn anchor_owner_child() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::process::CommandExt as _;
        let Some(root) = std::env::var_os("VELNOR_TEST_ANCHOR_ROOT") else {
            return Ok(());
        };
        let anchor = super::ProcessGroupAnchor::spawn()?;
        let mut command = if std::env::var_os("VELNOR_TEST_NESTED").is_some() {
            let mut command = std::process::Command::new(std::env::current_exe()?);
            command
                .args([
                    "--exact",
                    "command::process_group::tests::anchor_owner_child",
                    "--nocapture",
                ])
                .env_remove("VELNOR_TEST_NESTED");
            command
        } else {
            let mut command = std::process::Command::new("/bin/sh");
            command.args(["-c", "echo ready > \"$1/ready\"; (echo ready > \"$1/grandchild\"; sleep 2; echo escaped > \"$1/escaped\") & wait", "test"])
                .arg(root);
            command
        };
        let mut child = command.process_group(anchor.pgid()).spawn()?;
        let _status = child.wait()?;
        drop(anchor);
        Ok(())
    }

    #[test]
    fn anchor_drains_group_after_owner_is_killed() -> Result<(), Box<dyn std::error::Error>> {
        owner_death(false)
    }

    #[test]
    fn nested_anchor_drains_after_outer_owner_is_killed() -> Result<(), Box<dyn std::error::Error>>
    {
        owner_death(true)
    }

    fn owner_death(nested: bool) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::process::CommandExt as _;
        use std::time::{Duration, Instant};
        let temp = tempfile::tempdir()?;
        let mut owner_command = std::process::Command::new(std::env::current_exe()?);
        if nested {
            owner_command.env("VELNOR_TEST_NESTED", "1");
        }
        let mut owner = owner_command
            .args([
                "--exact",
                "command::process_group::tests::anchor_owner_child",
                "--nocapture",
            ])
            .env("VELNOR_TEST_ANCHOR_ROOT", temp.path())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !temp.path().join("ready").exists() || !temp.path().join("grandchild").exists() {
            if Instant::now() >= deadline {
                owner.kill()?;
                return Err("owner did not start".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut sentinel = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 3; echo alive > \"$1/sentinel\"", "test"])
            .arg(temp.path())
            .process_group(0)
            .spawn()?;
        owner.kill()?;
        owner.wait()?;
        if !sentinel.wait()?.success() {
            return Err("unrelated process failed".into());
        }
        assert!(temp.path().join("sentinel").exists());
        assert!(
            !temp.path().join("escaped").exists(),
            "descendant survived owner death"
        );
        Ok(())
    }
}
