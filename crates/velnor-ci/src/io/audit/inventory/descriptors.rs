// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::process::{Command, ExitStatus};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nix::sys::resource::{RLIM_INFINITY, Resource, getrlimit, setrlimit};

use crate::error::CiError;

const UNLIMITED_TARGET: u64 = 10_240;

pub(in crate::io::audit) struct OpenFileLimit {
    soft: u64,
    hard: u64,
}

struct Raised {
    holders: usize,
    original: (u64, u64),
}

static RAISED: Mutex<Raised> = Mutex::new(Raised {
    holders: 0,
    original: (0, 0),
});

fn state() -> MutexGuard<'static, Raised> {
    RAISED.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(in crate::io::audit) fn raise_open_file_limit() -> Option<OpenFileLimit> {
    let mut raised = state();
    if raised.holders == 0 {
        let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).ok()?;
        let target = if hard == RLIM_INFINITY {
            UNLIMITED_TARGET
        } else {
            hard
        };
        if target > soft {
            let _ = setrlimit(Resource::RLIMIT_NOFILE, target, hard);
        }
        raised.original = (soft, hard);
    }
    raised.holders += 1;
    let (soft, hard) = raised.original;
    Some(OpenFileLimit { soft, hard })
}

impl Drop for OpenFileLimit {
    fn drop(&mut self) {
        let mut raised = state();
        raised.holders -= 1;
        if raised.holders == 0 {
            let _ = setrlimit(Resource::RLIMIT_NOFILE, self.soft, self.hard);
        }
    }
}

const CONFINE: &str = "limit=$1; shift; ulimit -Sn \"$limit\" && exec \"$@\"";

impl OpenFileLimit {
    fn confine(&self, build: &Command) -> Command {
        let limit = if self.soft == RLIM_INFINITY {
            "unlimited".to_owned()
        } else {
            self.soft.to_string()
        };
        let mut confined = Command::new("sh");
        confined
            .args(["-c", CONFINE, "velnor-build", &limit])
            .arg(build.get_program())
            .args(build.get_args());
        if let Some(directory) = build.get_current_dir() {
            confined.current_dir(directory);
        }
        for (name, value) in build.get_envs() {
            match value {
                Some(value) => confined.env(name, value),
                None => confined.env_remove(name),
            };
        }
        confined
    }
}

pub(in crate::io::audit) fn run_build(
    limit: Option<&OpenFileLimit>,
    build: &mut Command,
) -> Result<ExitStatus, CiError> {
    let status = match limit {
        Some(limit) => limit.confine(build).status(),
        None => build.status(),
    };
    status.map_err(|e| CiError::new(format!("cannot run build command: {e}")))
}

pub(super) fn open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(soft, _)| soft)
}

pub(super) fn descriptor_shortage(error: &io::Error) -> Option<i32> {
    error
        .raw_os_error()
        .filter(|errno| matches!(*errno, libc::EMFILE | libc::ENFILE))
}

pub(super) fn shortage_message(errno: i32, depth: usize) -> String {
    let place = if depth == 0 {
        "at the scan root".to_owned()
    } else {
        format!("{depth} levels deep")
    };
    if errno == libc::ENFILE {
        return format!(
            "the system-wide open-file table is full while scanning {place}; free descriptors on this host or retry (raising the per-process open-file limit does not help)"
        );
    }
    let tree = if depth == 0 {
        "the scan root".to_owned()
    } else {
        format!("directory tree deeper than {depth} levels with pending siblings")
    };
    format!(
        "{tree} exceeds the open-file limit ({}); raise the open-file limit",
        open_file_limit()
    )
}

pub(super) fn comparison_message(errno: i32) -> String {
    if errno == libc::ENFILE {
        return "the system-wide open-file table is full while comparing a recorded inventory; free descriptors on this host or retry (raising the per-process open-file limit does not help)".to_owned();
    }
    format!(
        "the open-file limit ({}) was exhausted while comparing a recorded inventory; raise the open-file limit",
        open_file_limit()
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nix::sys::stat::Mode;
    use nix::unistd::mkfifo;

    use super::*;

    const LOW: u64 = 256;

    fn lowered() -> (u64, u64) {
        let (_, hard) = getrlimit(Resource::RLIMIT_NOFILE).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            hard > LOW,
            "the hard open-file limit {hard} leaves no room to raise the soft limit from {LOW}"
        );
        setrlimit(Resource::RLIMIT_NOFILE, LOW, hard).unwrap_or_else(|e| unreachable!("{e}"));
        (LOW, hard)
    }

    #[test]
    fn dropping_the_raise_restores_the_soft_limit_the_caller_had() {
        let (soft, _) = lowered();
        let raised = raise_open_file_limit();
        assert!(open_file_limit() > soft, "the limit was not raised");
        drop(raised);
        assert_eq!(open_file_limit(), soft);
    }

    #[test]
    fn a_build_child_gets_the_original_limit_while_the_parent_keeps_the_raised_one() {
        let (soft, _) = lowered();
        let Some(raised) = raise_open_file_limit() else {
            unreachable!("the limit can be read")
        };
        let during = open_file_limit();
        assert!(during > soft, "the limit was not raised");
        let scratch =
            std::env::temp_dir().join(format!("velnor-child-limit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap_or_else(|e| unreachable!("{e}"));
        let (seen, up, go) = (scratch.join("seen"), scratch.join("up"), scratch.join("go"));
        for fifo in [&up, &go] {
            mkfifo(fifo, Mode::S_IRWXU).unwrap_or_else(|e| unreachable!("{e}"));
        }
        let mut build = Command::new("sh");
        build
            .arg("-c")
            .arg("ulimit -Sn >\"$0\"; echo up >\"$1\"; read line <\"$2\" || :")
            .args([&seen, &up, &go]);
        let (parent, status) = std::thread::scope(|scope| {
            let child = scope.spawn(|| run_build(Some(&raised), &mut build));
            fs::read_to_string(&up).unwrap_or_else(|e| unreachable!("{e}"));
            let parent = open_file_limit();
            drop(fs::OpenOptions::new().write(true).open(&go));
            (parent, child.join())
        });
        let child_limit = fs::read_to_string(&seen).unwrap_or_else(|e| unreachable!("{e}"));
        fs::remove_dir_all(&scratch).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            parent, during,
            "the parent's limit changed while the child ran"
        );
        assert!(matches!(status, Ok(Ok(status)) if status.success()));
        assert_eq!(child_limit.trim(), soft.to_string());
        assert_eq!(open_file_limit(), during);
    }

    #[test]
    fn overlapping_raises_restore_the_limit_the_first_caller_had_once_the_last_drops() {
        let (soft, _) = lowered();
        let first = raise_open_file_limit();
        let second = raise_open_file_limit();
        drop(first);
        assert!(
            open_file_limit() > soft,
            "the first drop lowered the limit under the second holder"
        );
        drop(second);
        assert_eq!(open_file_limit(), soft);
    }
}
