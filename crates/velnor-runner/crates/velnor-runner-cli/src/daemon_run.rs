//! `daemon run` holds one lock and starts the host controller.

use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use velnor_runner_host::{DaemonLock, HostConfig, ListenFault, run_host_blocking};

const RETRY: Duration = Duration::from_secs(5);
const MAX_CONFIG_BYTES: u64 = 1_048_576;

/// Missing file waits. Valid TOML listens. Rejected TOML is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonIntent {
    /// `host.toml` is absent.
    Wait,
    /// Parsed config can drive the host controller.
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
    loop {
        if !lock.is_held() {
            return ExitCode::from(1);
        }
        let text = match read_config_file(&state.join("host.toml")) {
            Ok(Some(text)) => text,
            Ok(None) => {
                pause();
                continue;
            }
            Err(_) => {
                eprintln!("invalid config");
                pause();
                continue;
            }
        };
        let config = match HostConfig::parse(&text) {
            Ok(config) => config,
            Err(_) => {
                eprintln!("invalid config");
                pause();
                continue;
            }
        };
        return drive(state, &config);
    }
}

fn read_config_file(path: &Path) -> std::io::Result<Option<String>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_CONFIG_BYTES =>
        {
            metadata
        }
        Ok(_) => return Err(std::io::ErrorKind::InvalidData.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = std::fs::read_to_string(path)?;
    let length = u64::try_from(text.len()).map_err(|_| std::io::ErrorKind::InvalidData)?;
    if length > MAX_CONFIG_BYTES || length > metadata.len() {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(Some(text))
}

fn drive(state: &Path, config: &HostConfig) -> ExitCode {
    match run_host_blocking(config, state) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_listen_error(&error),
    }
}

fn report_listen_error(error: &ListenFault) -> ExitCode {
    eprintln!("{error}");
    ExitCode::from(1)
}

fn pause() {
    thread::sleep(RETRY);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{DaemonIntent, daemon_intent, read_config_file};
    use velnor_runner_host::{ConnectionId, DaemonLock, HostConfig, HostError};

    const SAMPLE: &str = concat!(
        "schema = 2\n",
        "[[connections]]\n",
        "connection_id = \"essential-mac\"\n",
        "repository_id = 123\n",
        "repository = \"tailrocks/velnor-new\"\n",
        "credential_ref = \"keychain:com.tailrocks.velnor.host/essential-mac\"\n",
        "[connections.scale_set]\n",
        "name = \"ubuntu-26.04-scale-set\"\n",
        "labels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\n",
        "[connections.source_policy]\n",
        "mode = \"branch_restricted\"\n",
        "branches = [\"main\"]\n",
        "tag_prefixes = []\n",
        "events = [\"push\"]\n",
        "pull_request_rules = []\n",
        "[host]\n",
        "max_jobs = 1\n",
        "[host.resources]\n",
        "runner_cpu_millicores = 1000\n",
        "runner_memory_bytes = 2147483648\n",
        "dind_cpu_millicores = 3000\n",
        "dind_memory_bytes = 6442450944\n",
        "operational_memory_reserve_bytes = 1073741824\n",
        "[docker]\n",
        "context = \"orbstack\"\n",
        "platform = \"linux/amd64\"\n",
        "endpoint = \"unix:///var/run/docker.sock\"\n",
    );

    #[test]
    fn missing_config_waits() {
        assert_eq!(daemon_intent(None), DaemonIntent::Wait);
    }

    #[test]
    fn daemon_reads_only_regular_bounded_config_files() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("velnor-config-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let path = dir.join("host.toml");
        if read_config_file(&path)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("missing config was not empty".to_owned());
        }
        std::fs::write(&path, SAMPLE).map_err(|error| error.to_string())?;
        if read_config_file(&path).map_err(|error| error.to_string())? != Some(SAMPLE.to_owned()) {
            return Err("regular config changed".to_owned());
        }
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink("missing", &path).map_err(|error| error.to_string())?;
        if read_config_file(&path).is_ok() {
            return Err("symlink config was accepted".to_owned());
        }
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        std::fs::write(&path, vec![b'x'; 1_048_577]).map_err(|error| error.to_string())?;
        if read_config_file(&path).is_ok() {
            return Err("oversized config was accepted".to_owned());
        }
        std::fs::remove_dir_all(&dir).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn valid_host_toml_listens() {
        assert_eq!(daemon_intent(Some(SAMPLE)), DaemonIntent::Listen);
    }

    #[test]
    fn daemon_passes_the_complete_config_to_the_host() -> Result<(), String> {
        let config = HostConfig::parse(SAMPLE).map_err(|error| error.to_string())?;
        let connection = ConnectionId::parse("essential-mac").map_err(|error| error.to_string())?;
        if config.connections().len() != 1
            || config.connection(&connection).is_none()
            || config.docker.endpoint != "unix:///var/run/docker.sock"
        {
            return Err("daemon config lost a connection or shared setting".to_owned());
        }
        Ok(())
    }

    #[test]
    fn bad_toml_is_err() {
        let bad = SAMPLE.replace("schema = 2", "schema = 2\ntoken = \"secret\"");
        assert_eq!(daemon_intent(Some(&bad)), DaemonIntent::Err);
    }

    #[test]
    fn old_single_repository_config_is_rejected() {
        let old = SAMPLE.replace("schema = 2", "schema = 1");
        assert_eq!(daemon_intent(Some(&old)), DaemonIntent::Err);
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
