// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::filesystem::atomic_write;
use super::quarantine::{open_file, safe_plist_metadata};
use velnor_runner_host::untrusted_path;

const ORB: &str = "orb";
const RUNNER: &str = "velnor-runner";
const FILE: &str = "service-environment.json";
const ORB_FIELD: &str = "orb";
const RUNNER_FIELD: &str = "runner_binary";
const LIMIT: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Environment {
    pub(super) orb: PathBuf,
    pub(super) runner_binary: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Missing {
    Orb,
    Runner,
    Unsafe(PathBuf),
}

pub(super) fn resolve(
    orb: Option<&Path>,
    runner_binary: Option<&Path>,
    host: &Path,
    search: Option<&OsStr>,
    installer: u32,
) -> Result<Environment, Missing> {
    let orb = match orb {
        Some(path) if executable(path) => path.to_path_buf(),
        Some(_) => return Err(Missing::Orb),
        None => search
            .into_iter()
            .flat_map(std::env::split_paths)
            .map(|dir| dir.join(ORB))
            .find(|candidate| executable(candidate))
            .ok_or(Missing::Orb)?,
    };
    refuse_untrusted(&orb, installer)?;
    let runner_binary = match runner_binary {
        Some(path) if executable(path) => path.to_path_buf(),
        Some(_) => return Err(Missing::Runner),
        None => host
            .parent()
            .map(|dir| dir.join(RUNNER))
            .filter(|path| executable(path))
            .ok_or(Missing::Runner)?,
    };
    refuse_untrusted(&runner_binary, installer)?;
    Ok(Environment { orb, runner_binary })
}

fn refuse_untrusted(path: &Path, installer: u32) -> Result<(), Missing> {
    untrusted_path(path, installer).map_or(Ok(()), |unsafe_path| Err(Missing::Unsafe(unsafe_path)))
}

pub(super) fn verify_recorded(state: &Path, installer: u32) -> Result<(), Missing> {
    let Some(environment) = installed(state) else {
        return Ok(());
    };
    refuse_untrusted(&environment.orb, installer)?;
    refuse_untrusted(&environment.runner_binary, installer)
}

pub(super) fn installed(state: &Path) -> Option<Environment> {
    let bytes = read_private(&state.join(FILE))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(Environment {
        orb: path_field(&value, ORB_FIELD)?,
        runner_binary: path_field(&value, RUNNER_FIELD)?,
    })
}

pub(super) fn record(state: &Path, environment: &Environment) -> bool {
    let (Some(orb), Some(runner)) = (environment.orb.to_str(), environment.runner_binary.to_str())
    else {
        return false;
    };
    let body = serde_json::json!({ ORB_FIELD: orb, RUNNER_FIELD: runner }).to_string();
    atomic_write(&state.join(FILE), body.as_bytes()).is_ok()
}

pub(super) fn snapshot(state: &Path) -> Option<Vec<u8>> {
    read_private(&state.join(FILE))
}

pub(super) fn restore(state: &Path, previous: Option<&[u8]>) {
    let path = state.join(FILE);
    match previous {
        Some(bytes) => {
            atomic_write(&path, bytes).ok();
        }
        None => {
            std::fs::remove_file(&path).ok();
        }
    }
}

fn read_private(path: &Path) -> Option<Vec<u8>> {
    let file = open_file(path).ok()?;
    if !safe_plist_metadata(&file.metadata().ok()?) {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (u64::try_from(bytes.len()).ok()? <= LIMIT).then_some(bytes)
}

fn path_field(value: &serde_json::Value, name: &str) -> Option<PathBuf> {
    Some(PathBuf::from(value.get(name)?.as_str()?))
}

fn executable(path: &Path) -> bool {
    path.is_absolute()
        && std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        && rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{Environment, FILE, Missing, installed, record, resolve, verify_recorded};

    fn me() -> u32 {
        rustix::process::geteuid().as_raw()
    }

    fn executable_file(path: &Path, mode: u32) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().ok_or(std::io::ErrorKind::InvalidInput)?)?;
        std::fs::write(path, "#!/bin/sh\n")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    #[test]
    fn recorded_environment_survives_a_read_back_for_start() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let state = temp.path().canonicalize()?;
        let environment = Environment {
            orb: state.join("a&b<c>").join("orb"),
            runner_binary: state.join("velnor-runner"),
        };
        assert!(record(&state, &environment));
        assert!(record(&state, &environment));
        assert_eq!(installed(&state), Some(environment));
        let mode = std::fs::metadata(state.join(FILE))?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        Ok(())
    }

    #[test]
    fn installed_refuses_a_fifo_without_blocking() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let state = temp.path().canonicalize()?;
        let made = std::process::Command::new("mkfifo")
            .arg(state.join(FILE))
            .status()?;
        assert!(made.success());
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || sender.send(installed(&state)));
        let outcome = receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
        assert_eq!(outcome, None);
        Ok(())
    }

    #[test]
    fn installed_refuses_a_record_open_to_others() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let state = temp.path().canonicalize()?;
        let environment = Environment {
            orb: state.join("orb"),
            runner_binary: state.join("velnor-runner"),
        };
        assert!(record(&state, &environment));
        std::fs::set_permissions(state.join(FILE), std::fs::Permissions::from_mode(0o644))?;
        assert_eq!(installed(&state), None);
        Ok(())
    }

    #[test]
    fn the_first_orb_on_path_that_this_user_cannot_run_is_skipped() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        executable_file(&root.join("first/orb"), 0o601)?;
        executable_file(&root.join("second/orb"), 0o755)?;
        executable_file(&root.join("host/velnor-runner"), 0o755)?;
        let search = std::env::join_paths([root.join("first"), root.join("second")])
            .map_err(|_| std::io::ErrorKind::InvalidInput)?;
        let resolved = resolve(
            None,
            None,
            &root.join("host/velnor-host"),
            Some(search.as_os_str()),
            me(),
        );
        assert_eq!(
            resolved.map(|environment| environment.orb),
            Ok(root.join("second/orb"))
        );
        Ok(())
    }

    #[test]
    fn the_default_runner_must_exist_and_be_executable() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let orb = root.join("orb");
        executable_file(&orb, 0o755)?;
        let host: PathBuf = root.join("host/velnor-host");
        let run = |orb: &Path| resolve(Some(orb), None, &host, None, me());
        assert_eq!(run(&orb), Err(Missing::Runner));
        executable_file(&root.join("host/velnor-runner"), 0o644)?;
        assert_eq!(run(&orb), Err(Missing::Runner));
        executable_file(&root.join("host/velnor-runner"), 0o755)?;
        assert_eq!(
            run(&orb).map(|environment| environment.runner_binary),
            Ok(root.join("host/velnor-runner"))
        );
        Ok(())
    }

    #[test]
    fn a_recorded_orb_that_became_group_writable_is_refused_at_start_and_stop()
    -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let (orb, runner) = (root.join("bin/orb"), root.join("bin/velnor-runner"));
        executable_file(&orb, 0o755)?;
        executable_file(&runner, 0o755)?;
        let state = root.join("state");
        std::fs::create_dir_all(&state)?;
        let environment = Environment {
            orb: orb.clone(),
            runner_binary: runner,
        };
        assert!(record(&state, &environment));
        assert_eq!(verify_recorded(&state, me()), Ok(()));
        std::fs::set_permissions(&orb, std::fs::Permissions::from_mode(0o775))?;
        assert_eq!(verify_recorded(&state, me()), Err(Missing::Unsafe(orb)));
        Ok(())
    }
}
