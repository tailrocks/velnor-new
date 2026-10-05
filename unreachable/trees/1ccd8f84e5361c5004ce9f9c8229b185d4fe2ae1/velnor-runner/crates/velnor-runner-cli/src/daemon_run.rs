//! `daemon run` holds one lock and calls `launch_once`.

use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use velnor_runner_host::{DaemonLock, HostConfig, LaunchReport, launch_blocking, load_secret};

use crate::dispatch::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE};

const RETRY: Duration = Duration::from_secs(5);

/// Missing file waits. Valid TOML listens. Rejected TOML is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonIntent {
    /// `host.toml` is absent.
    Wait,
    /// Parsed config can drive one launch.
    Listen,
    /// The file is not a valid host config.
    Err,
}

/// Classify `host.toml` text. `None` means the file is missing.
#[must_use]
pub(crate) fn daemon_intent(toml_text: Option<&str>) -> DaemonIntent {
    match toml_text {
        None => DaemonIntent::Wait,
        Some(text) => match HostConfig::parse(text) {
            Ok(_) => DaemonIntent::Listen,
            Err(_) => DaemonIntent::Err,
        },
    }
}

/// Hold `daemon.lock` and launch until the lock is lost.
pub(crate) fn run_daemon(state: &Path) -> ExitCode {
    let Ok(lock) = DaemonLock::try_acquire(&state.join("daemon.lock")) else {
        eprintln!("daemon already running");
        return ExitCode::from(1);
    };
    serve(state, &lock)
}

fn serve(state: &Path, lock: &DaemonLock) -> ExitCode {
    if !lock.is_held() {
        return ExitCode::from(1);
    }
    loop {
        if !lock.is_held() {
            return ExitCode::from(1);
        }
        step(state);
    }
}

fn step(state: &Path) {
    let text = std::fs::read_to_string(state.join("host.toml")).ok();
    match daemon_intent(text.as_deref()) {
        DaemonIntent::Wait => pause(),
        DaemonIntent::Err => {
            eprintln!("invalid config");
            pause();
        }
        DaemonIntent::Listen => listen_config(state, text.as_deref()),
    }
}

fn listen_config(state: &Path, text: Option<&str>) {
    let Some(text) = text else {
        pause();
        return;
    };
    let Ok(config) = HostConfig::parse(text) else {
        eprintln!("invalid config");
        pause();
        return;
    };
    drive(state, &config);
}

fn drive(state: &Path, config: &HostConfig) {
    let Some((owner, repo)) = split_repo(&config.github.repository) else {
        eprintln!("invalid config");
        pause();
        return;
    };
    let secret = match load_secret(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!("{error}");
            pause();
            return;
        }
    };
    let Some(pat) = credential_text(secret.as_slice()) else {
        eprintln!("keychain");
        pause();
        return;
    };
    match launch_blocking(
        pat,
        owner,
        repo,
        &config.docker.endpoint,
        &state.join("launch.db"),
        config.host.max_jobs,
    ) {
        Ok(report) => finish_launch(&report),
        Err(error) => {
            eprintln!("{error}");
            pause();
        }
    }
}

fn finish_launch(report: &LaunchReport) {
    println!("set_id={} workers={}", report.set_id, report.workers.len());
    if report.workers.is_empty() {
        pause();
    }
}

fn credential_text(secret: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(secret).ok()?;
    let pat = text.trim();
    if pat.is_empty() { None } else { Some(pat) }
}

fn split_repo(repository: &str) -> Option<(&str, &str)> {
    let (owner, repo) = repository.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner, repo))
}

fn pause() {
    thread::sleep(RETRY);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{DaemonIntent, daemon_intent};
    use velnor_runner_host::{DaemonLock, HostConfig, HostError};

    const SAMPLE: &str = concat!(
        "schema = 1\n",
        "[github]\n",
        "repository = \"tailrocks/velnor-new\"\n",
        "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
        "credential_ref = \"keychain:com.tailrocks.velnor.host/local\"\n",
        "[host]\n",
        "max_jobs = 1\n",
        "[docker]\n",
        "context = \"orbstack\"\n",
        "platform = \"linux/amd64\"\n",
        "endpoint = \"unix:///var/run/docker.sock\"\n",
    );

    #[test]
    fn credential_text_trims_and_rejects_empty() {
        assert_eq!(
            super::credential_text(b"canary-token\n"),
            Some("canary-token")
        );
        assert_eq!(super::credential_text(b"\n"), None);
        assert_eq!(super::credential_text(&[0xff, 0xfe]), None);
    }

    #[test]
    fn missing_config_waits() {
        assert_eq!(daemon_intent(None), DaemonIntent::Wait);
    }

    #[test]
    fn valid_host_toml_listens() {
        assert_eq!(daemon_intent(Some(SAMPLE)), DaemonIntent::Listen);
    }

    #[test]
    fn bad_toml_is_err_and_hides_the_secret() {
        let bad = SAMPLE.replace("schema = 1", "schema = 2\ntoken = \"secret\"");
        assert_eq!(daemon_intent(Some(&bad)), DaemonIntent::Err);
        let error = match HostConfig::parse(&bad) {
            Ok(_) => HostError::Config,
            Err(error) => error,
        };
        let text = format!("{error}");
        assert_eq!(text, "invalid config");
        assert!(!text.contains("secret"));
    }

    #[test]
    fn second_daemon_lock_fails_closed() -> Result<(), String> {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("velnor-daemon-lock-{}", std::process::id()));
        remove_tree(&dir);
        std::fs::create_dir_all(&dir).map_err(|_| "mkdir".to_owned())?;
        let path = dir.join("daemon.lock");
        let first = DaemonLock::try_acquire(&path).map_err(|_| "first".to_owned())?;
        if !first.is_held() {
            return Err("not held".to_owned());
        }
        match DaemonLock::try_acquire(&path) {
            Err(HostError::Lock) => {}
            Ok(_) => return Err("second acquired".to_owned()),
            Err(_) => return Err("other".to_owned()),
        }
        drop(first);
        remove_tree(&dir);
        Ok(())
    }

    fn remove_tree(dir: &std::path::Path) {
        match std::fs::remove_dir_all(dir) {
            Ok(()) | Err(_) => {}
        }
    }
}
