// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::bounded::{Cancel, Outcome, capture_streams};

const PREFIX: &str = "velnor-nextest-list";
const PROBE: &str = "velnor-exec-probe";
const NOT_EXECUTABLE: i32 = 126;
const PROBE_LIMIT: Duration = Duration::from_secs(10);
const PROBE_OUTPUT: usize = 4096;
const SHELL: &str = "/bin/sh";
const PROBE_SCRIPT: &str = "printf '#!/bin/sh\\nexit 0\\n' > \"$1\" && /bin/chmod 755 \"$1\" && \"$1\"; status=$?; /bin/rm -f \"$1\"; exit $status";

pub(super) type ExecProbe = dyn Fn(&Path) -> io::Result<bool>;

pub(super) fn can_execute_from(directory: &Path) -> io::Result<bool> {
    probe(PROBE_SCRIPT, directory, PROBE_LIMIT)
}

fn probe(script: &str, directory: &Path, limit: Duration) -> io::Result<bool> {
    let mut command = Command::new(SHELL);
    command
        .env_clear()
        .args(["-c", script, PROBE])
        .arg(directory.join(PROBE));
    let at = Instant::now() + limit;
    let outcome = capture_streams(
        &mut command,
        PROBE_OUTPUT,
        PROBE_OUTPUT,
        &|| Some(at),
        &Cancel::new(),
    )?;
    let output = match outcome {
        Outcome::Exited(output) => output,
        Outcome::TimedOut | Outcome::Lingering | Outcome::Cancelled => {
            return Err(io::Error::other(format!(
                "the check did not finish within {} s",
                limit.as_secs_f64()
            )));
        }
        Outcome::Oversized => return Err(io::Error::other("the check printed too much")),
    };
    match output.status.code() {
        Some(0) => Ok(true),
        Some(NOT_EXECUTABLE) => Ok(false),
        _ => Err(io::Error::other(format!(
            "{}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

pub(super) struct ListDir(PathBuf);

impl ListDir {
    pub(super) fn create(
        runner_temp: Option<OsString>,
        process: u32,
        can_execute: &ExecProbe,
    ) -> Result<Self, String> {
        let temp = runner_temp
            .filter(|temp| !temp.is_empty())
            .ok_or("RUNNER_TEMP is not set")?;
        let directory = PathBuf::from(temp).join(format!("{PREFIX}-{process}"));
        match fs::remove_dir_all(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot clear {}: {error}", directory.display())),
        }
        fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let created = Self(directory);
        match can_execute(&created.0) {
            Ok(true) => Ok(created),
            Ok(false) => Err(format!(
                "{} does not allow executing files (mounted noexec?), so the extracted test binaries could not run there",
                created.0.display()
            )),
            Err(error) => Err(format!(
                "cannot check that {} allows executing files: {error}",
                created.0.display()
            )),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ListDir {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "velnor-ci: cannot remove the early nextest list directory {}: {error}; the extracted archive stays on disk",
                self.0.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ListDir, can_execute_from, probe};
    use std::ffi::OsString;
    use std::io;

    #[test]
    fn creates_a_directory_under_the_runner_temp_and_removes_it_on_drop() {
        let temp = std::env::temp_dir().join(format!("velnor-list-dir-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let directory = ListDir::create(Some(OsString::from(&temp)), 7, &|_| Ok(true))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let path = directory.path().to_path_buf();
        assert_eq!(path, temp.join("velnor-nextest-list-7"));
        assert!(path.is_dir());
        drop(directory);
        assert!(!path.exists());
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn starts_from_an_empty_directory_when_an_earlier_one_was_left() {
        let temp = std::env::temp_dir().join(format!("velnor-list-stale-{}", std::process::id()));
        let stale = temp.join("velnor-nextest-list-9");
        std::fs::create_dir_all(&stale).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(stale.join("old"), "x").unwrap_or_else(|e| unreachable!("{e}"));
        let directory = ListDir::create(Some(OsString::from(&temp)), 9, &|_| Ok(true))
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(!directory.path().join("old").exists());
        drop(directory);
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn refuses_a_missing_or_empty_runner_temp() {
        assert!(ListDir::create(None, 1, &|_| Ok(true)).is_err());
        assert!(ListDir::create(Some(OsString::new()), 1, &|_| Ok(true)).is_err());
    }

    #[test]
    fn refuses_a_directory_that_does_not_allow_execution_and_removes_it() {
        let temp = std::env::temp_dir().join(format!("velnor-list-noexec-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let error = ListDir::create(Some(OsString::from(&temp)), 3, &|_| Ok(false))
            .err()
            .unwrap_or_else(|| unreachable!("a noexec directory was accepted"));
        let expected = temp.join("velnor-nextest-list-3");
        assert!(error.contains(&expected.display().to_string()), "{error}");
        assert!(error.contains("does not allow executing files"), "{error}");
        assert!(!expected.exists());
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn reports_a_failed_execution_check_with_its_cause() {
        let temp = std::env::temp_dir().join(format!("velnor-list-probe-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let error = ListDir::create(Some(OsString::from(&temp)), 4, &|_| {
            Err(io::Error::other("probe broke"))
        })
        .err()
        .unwrap_or_else(|| unreachable!("a failed check was accepted"));
        assert!(error.contains("cannot check that"), "{error}");
        assert!(error.contains("probe broke"), "{error}");
        assert!(!temp.join("velnor-nextest-list-4").exists());
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn the_real_probe_runs_a_file_from_an_ordinary_directory_and_leaves_nothing() {
        let temp = std::env::temp_dir().join(format!("velnor-list-real-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(can_execute_from(&temp).unwrap_or_else(|e| unreachable!("{e}")));
        assert_eq!(std::fs::read_dir(&temp).map(Iterator::count).ok(), Some(0));
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn the_real_probe_is_not_defeated_by_processes_spawned_concurrently() {
        let temp = std::env::temp_dir().join(format!("velnor-list-race-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let spawners: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..200 {
                        drop(
                            std::process::Command::new("/bin/sh")
                                .args(["-c", ":"])
                                .status(),
                        );
                    }
                })
            })
            .collect();
        let probers: Vec<_> = (0..4)
            .map(|index| {
                let own = temp.join(index.to_string());
                std::fs::create_dir_all(&own).unwrap_or_else(|e| unreachable!("{e}"));
                std::thread::spawn(move || {
                    (0..50).all(|_| matches!(can_execute_from(&own), Ok(true)))
                })
            })
            .collect();
        for spawner in spawners {
            spawner
                .join()
                .unwrap_or_else(|_| unreachable!("spawner panicked"));
        }
        for prober in probers {
            assert!(
                prober
                    .join()
                    .unwrap_or_else(|_| unreachable!("prober panicked"))
            );
        }
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_probe_that_hangs_is_stopped_with_its_descendants_and_reported() {
        let temp = std::env::temp_dir().join(format!("velnor-list-hang-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let started = std::time::Instant::now();
        let error = probe(
            "sleep 30 & echo $! > \"$1\"; wait",
            &temp,
            std::time::Duration::from_millis(300),
        )
        .err()
        .unwrap_or_else(|| unreachable!("a hanging probe was accepted"));
        assert!(
            error.to_string().contains("did not finish within 0.3 s"),
            "{error}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(9));
        let pid = std::fs::read_to_string(temp.join("velnor-exec-probe"))
            .unwrap_or_else(|e| unreachable!("{e}"))
            .trim()
            .to_owned();
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", &pid])
                .output()
                .is_ok_and(|output| output.status.success())
        };
        while alive() && std::time::Instant::now() < limit {
            std::thread::yield_now();
        }
        assert!(!alive(), "the probe's descendant {pid} survived");
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn the_real_probe_fails_loudly_for_a_missing_directory() {
        let missing = std::env::temp_dir().join(format!("velnor-list-gone-{}", std::process::id()));
        assert!(can_execute_from(&missing).is_err());
    }
}
