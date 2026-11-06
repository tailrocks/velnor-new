// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Seek;
use std::time::{Duration, Instant};

use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

use super::{Archive, append_log, create_private_file, io_error};
use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::MAX_ARCHIVE_BYTES;
use crate::internal::internal;

const CURL: &str = "/usr/bin/curl";
const PROCESS_OUTPUT_BYTES: usize = 64 * 1024;
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(300);

pub(super) fn download_archive(archive: &Archive, log: &mut File) -> Result<(), OrchestratorError> {
    let result = download_owned_archive(archive, log);
    if let Err(error) = &result {
        append_log(log, error.to_string().as_bytes())?;
        append_log(log, b"\n")?;
    }
    result
}

fn download_owned_archive(archive: &Archive, log: &mut File) -> Result<(), OrchestratorError> {
    let mut file = create_private_file(&archive.path)?;
    let deadline = CheckDeadline::after(ARCHIVE_TIMEOUT)
        .map_err(|error| internal(&format!("native_validator_archive_deadline:{error}")))?;
    let args: Vec<OsString> = [
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "15",
        "--max-time",
        "300",
        "--max-filesize",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([
        OsString::from(MAX_ARCHIVE_BYTES.to_string()),
        OsString::from(archive.url.as_str()),
    ])
    .collect();
    let command = IsolatedCommand::repo_task(CURL, args, &[])
        .map_err(|error| internal(&format!("native_validator_curl_command:{error}")))?;
    for attempt in 0..4 {
        let (output, _drained) = command
            .run_quiescent_stdout_to_file_until(
                &mut file,
                MAX_ARCHIVE_BYTES,
                PROCESS_OUTPUT_BYTES,
                deadline,
            )
            .map_err(|error| {
                internal(&format!("native_validator_archive_download_failed:{error}"))
            })?;
        append_log(log, &output.stderr)?;
        if output.success {
            let metadata = fs::symlink_metadata(&archive.path)
                .map_err(|error| io_error(&archive.path, error))?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() > MAX_ARCHIVE_BYTES
            {
                return Err(internal("native_validator_archive_download_unsafe"));
            }
            return Ok(());
        }
        if attempt == 3 || output.code == Some(63) || output.code.is_none() {
            break;
        }
        let delay = Duration::from_secs(1);
        if !deadline
            .remaining()
            .is_ok_and(|remaining| remaining > delay)
        {
            return Err(internal("native_validator_archive_download_deadline"));
        }
        std::thread::sleep(delay);
        if deadline
            .remaining()
            .and_then(|remaining| {
                CheckDeadline::from_start_with_cleanup_reserve(Instant::now(), remaining)
            })
            .is_err()
        {
            return Err(internal("native_validator_archive_download_deadline"));
        }
        file.set_len(0)
            .map_err(|error| io_error(&archive.path, error))?;
        file.rewind()
            .map_err(|error| io_error(&archive.path, error))?;
    }
    Err(internal("native_validator_archive_download_failed"))
}
