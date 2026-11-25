// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;

#[cfg(target_os = "linux")]
mod tree {
    use std::fs;
    use std::thread;
    use std::time::{Duration, Instant};

    use nix::sys::prctl::set_child_subreaper;
    use nix::sys::signal::{Signal, kill};
    use nix::sys::wait::{WaitPidFlag, waitpid};
    use nix::unistd::Pid;

    use crate::error::CiError;

    const LIMIT: Duration = Duration::from_secs(10);
    const STEP: Duration = Duration::from_millis(5);

    pub(super) fn adopt() -> Result<(), CiError> {
        set_child_subreaper(true)
            .map_err(|error| CiError::new(format!("cannot adopt orphaned processes: {error}")))
    }

    pub(super) fn reap() -> Result<(), CiError> {
        let deadline = Instant::now() + LIMIT;
        loop {
            let children = children()?;
            if children.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(CiError::new(
                    "A process started by build-test.sh could not be stopped.",
                ));
            }
            for child in children {
                kill(child, Signal::SIGKILL).ok();
                waitpid(child, Some(WaitPidFlag::WNOHANG)).ok();
            }
            thread::sleep(STEP);
        }
    }

    fn children() -> Result<Vec<Pid>, CiError> {
        let unreadable = |error: std::io::Error| {
            CiError::new(format!("cannot list processes in /proc: {error}"))
        };
        let me = std::process::id();
        let mut found = Vec::new();
        for entry in fs::read_dir("/proc").map_err(unreadable)? {
            let name = entry.map_err(unreadable)?.file_name();
            let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
                continue;
            };
            if parent_of(pid) == Some(me)
                && let Ok(raw) = i32::try_from(pid)
            {
                found.push(Pid::from_raw(raw));
            }
        }
        Ok(found)
    }

    fn parent_of(pid: u32) -> Option<u32> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }
}

#[cfg(not(target_os = "linux"))]
mod tree {
    use crate::error::CiError;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "same signature as the Linux implementation"
    )]
    pub(super) fn adopt() -> Result<(), CiError> {
        Ok(())
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "same signature as the Linux implementation"
    )]
    pub(super) fn reap() -> Result<(), CiError> {
        Ok(())
    }
}

pub(super) fn adopt() -> Result<(), CiError> {
    tree::adopt()
}

pub(super) fn reap() -> Result<(), CiError> {
    tree::reap()
}
