//! Process-shared target ownership for nested Cargo builds in CLI tests.

use std::error::Error;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const NESTED_CARGO_TARGET: &str = "velnor-cli-nested-cargo";

type Outcome<T> = Result<T, Box<dyn Error>>;

/// Give nested test-owned Cargo builds a shared target below the outer target.
/// These builds run while Nextest launches the outer test binary, so they must
/// not replace its `target/debug` executables.
pub(crate) fn nested_cargo_target_dir_for(outer_target: &Path) -> PathBuf {
    outer_target.join(NESTED_CARGO_TARGET)
}

/// Nested target for scripts that inherit Cargo's active target directory.
pub(crate) fn nested_cargo_target_dir() -> Outcome<PathBuf> {
    let outer = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            }
        }
        None => crate::impl_repo_policy::repo_root().join("target"),
    };
    std::fs::create_dir_all(&outer)?;
    Ok(nested_cargo_target_dir_for(&outer.canonicalize()?))
}

/// Acquire the OS lock for nested Cargo writers under `outer`.
///
/// The sibling lockfile remains outside the nested build target and is never
/// removed, so cleanup cannot split lock ownership across different inodes.
pub(crate) fn lock_nested_cargo_target_for(outer: &Path) -> Outcome<File> {
    std::fs::create_dir_all(outer)?;
    let lock_path = outer
        .canonicalize()?
        .join(format!("{NESTED_CARGO_TARGET}.lock"));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock()?;
    Ok(lock)
}

/// Acquire the process-shared lock for scripts inheriting this test target.
pub(crate) fn lock_nested_cargo_target() -> Outcome<File> {
    let nested = nested_cargo_target_dir()?;
    let outer = nested.parent().ok_or("nested Cargo target has no parent")?;
    lock_nested_cargo_target_for(outer)
}

#[cfg(test)]
mod nested_target_lock_tests {
    use std::error::Error;
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::process::{Child, Command, ExitStatus};
    use std::time::{Duration, Instant};

    use super::{NESTED_CARGO_TARGET, Outcome, lock_nested_cargo_target_for};

    #[test]
    fn nested_target_lock_serializes_processes_and_releases_on_exit() -> Outcome<()> {
        let outer = crate::impl_cli_tmp::fresh_tempdir("nested-lock")?;
        let lock_path = outer.join(format!("{NESTED_CARGO_TARGET}.lock"));
        let contender = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        let ready = outer.join("child-locked");
        let mut child = match spawn_lock_child(&outer, &ready) {
            Ok(child) => child,
            Err(error) => {
                drop(contender);
                crate::impl_cli_tmp::cleanup(&outer);
                return Err(error);
            }
        };
        if let Err(error) = wait_for_lock_child(&mut child, &ready) {
            return Err(stop_child_and_cleanup(&mut child, contender, &outer, error));
        }
        let contention = contender.try_lock();
        let blocked_while_child_holds_lock = match contention {
            Err(std::fs::TryLockError::WouldBlock) => true,
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(stop_child_and_cleanup(
                    &mut child,
                    contender,
                    &outer,
                    Box::new(error),
                ));
            }
            Ok(()) => false,
        };
        let child_status = match terminate_lock_child(&mut child) {
            Ok(status) => status,
            Err(error) => {
                drop(contender);
                return Err(format!(
                    "could not reap nested target lock child; fixture retained at {}: {error}",
                    outer.display()
                )
                .into());
            }
        };
        let released_after_child_exit = match contender.try_lock() {
            Ok(()) => true,
            Err(std::fs::TryLockError::WouldBlock) => false,
            Err(std::fs::TryLockError::Error(error)) => {
                drop(contender);
                crate::impl_cli_tmp::cleanup(&outer);
                return Err(error.into());
            }
        };
        drop(contender);
        crate::impl_cli_tmp::cleanup(&outer);
        assert!(
            blocked_while_child_holds_lock,
            "second process acquired held lock"
        );
        assert!(
            !child_status.success(),
            "lock holder exited before termination"
        );
        assert!(
            released_after_child_exit,
            "OS did not release lock after process exit"
        );
        Ok(())
    }

    #[test]
    fn nested_target_lock_child_holds_until_killed() -> Outcome<()> {
        let Some(outer) = std::env::var_os("VELNOR_TEST_NESTED_LOCK_OUTER") else {
            return Ok(());
        };
        let _lock = lock_nested_cargo_target_for(Path::new(&outer))?;
        let ready = std::env::var_os("VELNOR_TEST_NESTED_LOCK_READY")
            .ok_or("lock child ready path is missing")?;
        std::fs::write(ready, b"locked")?;
        std::thread::park_timeout(Duration::from_secs(30));
        Ok(())
    }

    fn spawn_lock_child(outer: &Path, ready: &Path) -> Outcome<Child> {
        Ok(Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "impl_cli_tmp::nested_target::nested_target_lock_tests::nested_target_lock_child_holds_until_killed",
                "--nocapture",
            ])
            .env("VELNOR_TEST_NESTED_LOCK_OUTER", outer)
            .env("VELNOR_TEST_NESTED_LOCK_READY", ready)
            .spawn()?)
    }

    fn terminate_lock_child(child: &mut Child) -> Outcome<ExitStatus> {
        if let Err(error) = child.kill() {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            return Err(error.into());
        }
        Ok(child.wait()?)
    }

    fn wait_for_lock_child(child: &mut Child, ready: &Path) -> Outcome<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if ready.is_file() {
                return Ok(());
            }
            if let Some(status) = child.try_wait()? {
                return Err(format!("lock child exited before acquiring lock: {status}").into());
            }
            if Instant::now() >= deadline {
                return Err("timed out waiting for child to acquire nested target lock".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop_child_and_cleanup(
        child: &mut Child,
        contender: std::fs::File,
        outer: &Path,
        cause: Box<dyn Error>,
    ) -> Box<dyn Error> {
        let cleanup_result = terminate_lock_child(child);
        drop(contender);
        match cleanup_result {
            Ok(_) => {
                crate::impl_cli_tmp::cleanup(outer);
                cause
            }
            Err(error) => format!(
                "{cause}; could not reap child, fixture retained at {}: {error}",
                outer.display()
            )
            .into(),
        }
    }
}
