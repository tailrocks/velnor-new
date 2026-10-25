use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{BoundedProcessError, Capture, timeout_error};

pub(super) struct GroupAnchor {
    child: Child,
    input: Option<ChildStdin>,
    pid: Pid,
}

impl GroupAnchor {
    pub(super) fn spawn(deadline: Instant) -> Result<Self, BoundedProcessError> {
        let mut child = Command::new("/bin/cat")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(BoundedProcessError::Io)?;
        let pid = Pid::from_child(&child);
        let input = child.stdin.take();
        let mut anchor = Self { child, input, pid };
        if anchor.input.is_none() {
            let error = io::Error::other("process group anchor has no stdin pipe");
            return match anchor.stop(deadline) {
                Ok(()) => Err(BoundedProcessError::Io(error)),
                Err(cleanup_error) => Err(BoundedProcessError::Cleanup(cleanup_error)),
            };
        }
        Ok(anchor)
    }

    pub(super) fn pid(&self) -> Pid {
        self.pid
    }

    pub(super) fn ensure_alive(&self) -> io::Result<()> {
        if leader_exited(self.pid)? {
            Err(io::Error::other("process group anchor exited unexpectedly"))
        } else {
            Ok(())
        }
    }

    pub(super) fn stop(&mut self, deadline: Instant) -> io::Result<()> {
        let mut error = self.signal_group_once().err();
        if error.is_some()
            && let Err(kill_error) = self.child.kill()
        {
            error.get_or_insert(kill_error);
        }
        self.input.take();

        let mut reaped = false;
        while !reaped {
            match reap_if_exited(&mut self.child, self.pid) {
                Ok(Some(_)) => reaped = true,
                Ok(None) => {}
                Err(reap_error) => {
                    error.get_or_insert(reap_error);
                }
            }
            if reaped {
                break;
            }
            if Instant::now() >= deadline {
                return Err(error.unwrap_or_else(timeout_error));
            }
            pause_until(deadline);
        }
        if Instant::now() >= deadline {
            return Err(error.unwrap_or_else(timeout_error));
        }
        error.map_or(Ok(()), Err)
    }

    pub(super) fn stop_with_target(
        &mut self,
        target: &mut Child,
        target_pid: Pid,
        mut capture: Option<&mut Capture>,
        deadline: Instant,
    ) -> io::Result<ExitStatus> {
        let mut error = self.signal_group_once().err();
        if error.is_some()
            && let Err(kill_error) = target.kill()
        {
            error.get_or_insert(kill_error);
        }
        if error.is_some()
            && let Err(kill_error) = self.child.kill()
        {
            error.get_or_insert(kill_error);
        }
        self.input.take();

        let mut target_reaped = false;
        let mut target_status = None;
        let mut anchor_reaped = false;
        while !target_reaped
            || !anchor_reaped
            || capture.as_ref().is_some_and(|item| !item.closed())
        {
            if let Some(item) = capture.as_deref_mut()
                && let Err(drain_error) = item.drain()
            {
                error.get_or_insert(drain_error);
            }
            if !target_reaped {
                match reap_if_exited(target, target_pid) {
                    Ok(Some(status)) => {
                        target_status = Some(status);
                        target_reaped = true;
                    }
                    Ok(None) => {}
                    Err(reap_error) => {
                        error.get_or_insert(reap_error);
                    }
                }
            }
            if !anchor_reaped {
                match reap_if_exited(&mut self.child, self.pid) {
                    Ok(Some(_)) => anchor_reaped = true,
                    Ok(None) => {}
                    Err(reap_error) => {
                        error.get_or_insert(reap_error);
                    }
                }
            }
            if target_reaped && anchor_reaped && capture.as_ref().is_none_or(|item| item.closed()) {
                if Instant::now() >= deadline {
                    return Err(error.unwrap_or_else(timeout_error));
                }
                break;
            }
            if Instant::now() >= deadline {
                return Err(error.unwrap_or_else(timeout_error));
            }
            pause_until(deadline);
        }
        if let Some(error) = error {
            Err(error)
        } else {
            target_status
                .ok_or_else(|| io::Error::other("target process exited without a wait status"))
        }
    }

    fn signal_group_once(&self) -> io::Result<()> {
        let liveness_error = self.ensure_alive().err();
        let signal_error = kill_process_group(self.pid, Signal::KILL)
            .err()
            .map(io::Error::from);
        match (liveness_error, signal_error) {
            (None, None) => Ok(()),
            (Some(error), None) | (None, Some(error)) => Err(error),
            (Some(liveness), Some(signal)) => Err(io::Error::new(
                signal.kind(),
                format!("{liveness}; process-group signal failed: {signal}"),
            )),
        }
    }
}

fn reap_if_exited(child: &mut Child, pid: Pid) -> io::Result<Option<ExitStatus>> {
    if leader_exited(pid)? {
        child.wait().map(Some)
    } else {
        Ok(None)
    }
}

fn leader_exited(pid: Pid) -> io::Result<bool> {
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    match waitid(WaitId::Pid(pid), options) {
        Ok(status) => Ok(status.is_some()),
        Err(rustix::io::Errno::INTR) => Ok(false),
        Err(error) => Err(io::Error::from(error)),
    }
}

fn pause_until(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(Duration::from_millis(4)));
    }
}
