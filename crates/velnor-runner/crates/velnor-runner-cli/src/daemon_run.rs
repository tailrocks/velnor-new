// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use velnor_runner_host::{DaemonLock, HostConfig, ListenFault, run_host_blocking};

const RETRY: Duration = Duration::from_secs(5);
const MAX_CONFIG_BYTES: u64 = 1_048_576;

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
        let Ok(config) = HostConfig::parse(&text) else {
            eprintln!("invalid config");
            pause();
            continue;
        };
        return drive(state, &config, lock);
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

fn drive(state: &Path, config: &HostConfig, lock: &DaemonLock) -> ExitCode {
    match run_host_blocking(config, state, lock) {
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
