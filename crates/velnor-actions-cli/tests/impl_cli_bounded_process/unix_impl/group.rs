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

    pub(super) fn ensure_alive(&self, deadline: Instant) -> io::Result<()> {
        if Instant::now() >= deadline {
            return Err(timeout_error());
        }
        if observe_process(self.pid, deadline)? == ProcessState::Exited {
            Err(io::Error::other("process group anchor exited unexpectedly"))
        } else {
            Ok(())
        }
    }

    pub(super) fn stop(&mut self, deadline: Instant) -> io::Result<()> {
        let mut error = self.signal_group_once(deadline).err();
        self.input.take();

        let mut reaped = false;
        while !reaped {
            match reap_if_exited(&mut self.child, self.pid, deadline) {
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
        let mut error = self.signal_group_once(deadline).err();
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
                match reap_if_exited(target, target_pid, deadline) {
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
                match reap_if_exited(&mut self.child, self.pid, deadline) {
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

    fn signal_group_once(&self, deadline: Instant) -> io::Result<()> {
        signal_owned_group_once(
            || {
                if Instant::now() >= deadline {
                    return Err(timeout_error());
                }
                let state = observe_process(self.pid, deadline)?;
                if Instant::now() >= deadline {
                    return Err(timeout_error());
                }
                Ok(state)
            },
            || {
                if Instant::now() >= deadline {
                    return Err(timeout_error());
                }
                kill_process_group(self.pid, Signal::KILL).map_err(io::Error::from)
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessState {
    Running,
    Exited,
}

fn signal_owned_group_once(
    observe: impl FnOnce() -> io::Result<ProcessState>,
    signal: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    match observe()? {
        ProcessState::Running | ProcessState::Exited => signal(),
    }
}

fn reap_if_exited(
    child: &mut Child,
    pid: Pid,
    deadline: Instant,
) -> io::Result<Option<ExitStatus>> {
    if observe_process(pid, deadline)? == ProcessState::Exited {
        child.wait().map(Some)
    } else {
        Ok(None)
    }
}

pub(super) fn observe_process(pid: Pid, deadline: Instant) -> io::Result<ProcessState> {
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    observe_with_retry(deadline, || match waitid(WaitId::Pid(pid), options) {
        Ok(Some(_)) => Ok(ProcessState::Exited),
        Ok(None) => Ok(ProcessState::Running),
        Err(error) => Err(io::Error::from(error)),
    })
}

fn observe_with_retry(
    deadline: Instant,
    mut observe: impl FnMut() -> io::Result<ProcessState>,
) -> io::Result<ProcessState> {
    loop {
        match observe() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                if Instant::now() >= deadline {
                    return Err(timeout_error());
                }
                pause_until(deadline);
            }
            result => return result,
        }
    }
}

fn pause_until(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(Duration::from_millis(4)));
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessState, observe_with_retry, signal_owned_group_once};
    use std::cell::Cell;
    use std::io;
    use std::time::{Duration, Instant};

    #[test]
    fn echild_for_anchor_never_signals_group() {
        let signals = Cell::new(0);
        let result = signal_owned_group_once(
            || Err(io::Error::from(rustix::io::Errno::CHILD)),
            || {
                signals.set(signals.get() + 1);
                Ok(())
            },
        );
        assert_eq!(
            result
                .expect_err("lost anchor ownership must fail")
                .raw_os_error(),
            Some(rustix::io::Errno::CHILD.raw_os_error())
        );
        assert_eq!(signals.get(), 0);
    }

    #[test]
    fn owned_running_or_unreaped_exit_allows_only_one_callers_signal() {
        for state in [ProcessState::Running, ProcessState::Exited] {
            let signals = Cell::new(0);
            signal_owned_group_once(
                || Ok(state),
                || {
                    signals.set(signals.get() + 1);
                    Ok(())
                },
            )
            .expect("owned process permits one group signal");
            assert_eq!(signals.get(), 1);
        }
    }

    #[test]
    fn interrupted_observation_retries_only_until_deadline() {
        let calls = Cell::new(0);
        let result = observe_with_retry(Instant::now() + Duration::from_millis(20), || {
            calls.set(calls.get() + 1);
            Err(io::ErrorKind::Interrupted.into())
        });
        assert_eq!(
            result
                .expect_err("interrupted observation must expire")
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(calls.get() > 0);
        assert!(calls.get() <= 6);
    }

    #[test]
    fn interrupted_observation_can_retry_before_deadline() {
        let calls = Cell::new(0);
        let result = observe_with_retry(Instant::now() + Duration::from_millis(100), || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                Ok(ProcessState::Running)
            }
        });
        assert_eq!(
            result.expect("second observation returns process state"),
            ProcessState::Running
        );
        assert_eq!(calls.get(), 2);
    }
}
