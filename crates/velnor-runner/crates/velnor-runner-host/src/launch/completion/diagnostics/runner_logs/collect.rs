use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::ExecInspectResponse;
use futures_util::StreamExt;
use tar::{Builder, EntryType, Header};
use tokio::time::{Instant as TokioInstant, sleep, sleep_until};

use crate::error::HostError;

use super::super::{CAPTURE_DEADLINE, check_capture_state};
use super::MAX_ARCHIVE_BYTES;

const EXEC_USER: &str = "1000:1000";
const CANCEL_POLL: Duration = Duration::from_millis(25);

const ARCHIVE_SCRIPT: &str = r#"set -eu
diag=/home/runner/_diag
[ ! -L "$diag" ] || exit 24
[ -e "$diag" ] || exit 3
[ -d "$diag" ] || exit 24
command -v find >/dev/null
command -v head >/dev/null
command -v stat >/dev/null
command -v tar >/dev/null
set -o pipefail
mapfile -d '' -t files < <(find "$diag" -mindepth 1 -maxdepth 1 -type f \
    \( -name 'Runner_*-utc.log' -o -name 'Worker_*-utc.log' \) -print0 | head -z -n 9)
[ "${#files[@]}" -le 8 ] || exit 24
wait "$!" || exit 24
total=0
set --
for file in "${files[@]}"; do
    [ -f "$file" ] && [ ! -L "$file" ] || exit 24
    name=${file##*/}
    case "$name" in
        Runner_[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9]-utc.log|Worker_[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9]-utc.log) ;;
        *) continue ;;
    esac
    size=$(stat -c %s -- "$file")
    case "$size" in ''|*[!0-9]*) exit 24 ;; esac
    [ "$size" -le 16777216 ] || exit 24
    total=$((total + size))
    [ "$total" -le 33554432 ] || exit 24
    set -- "$@" "$name"
done
/usr/bin/tar --format=ustar --no-recursion -C "$diag" -cf - . "$@"
"#;

pub(in crate::launch) fn create_exec_options() -> CreateExecOptions<String> {
    CreateExecOptions {
        attach_stdin: Some(false),
        attach_stdout: Some(true),
        attach_stderr: Some(false),
        tty: Some(false),
        detach_keys: None,
        env: Some(vec!["PATH=/usr/bin:/bin".to_owned()]),
        cmd: Some(vec![
            "/bin/bash".to_owned(),
            "-c".to_owned(),
            ARCHIVE_SCRIPT.to_owned(),
        ]),
        privileged: Some(false),
        user: Some(EXEC_USER.to_owned()),
        working_dir: Some("/home/runner".to_owned()),
    }
}

pub(in crate::launch) fn start_exec_options() -> StartExecOptions {
    StartExecOptions {
        detach: false,
        tty: false,
        output_capacity: Some(16 * 1024),
    }
}

pub(in crate::launch) async fn collect_exec_output(
    results: StartExecResults,
    started: Instant,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, HostError> {
    let StartExecResults::Attached { mut output, input } = results else {
        return Err(HostError::Ownership);
    };
    drop(input);
    let deadline = started
        .checked_add(CAPTURE_DEADLINE)
        .ok_or(HostError::DockerTimeout)?;
    let mut archive = Vec::new();
    loop {
        check_capture_state(started, deadline, cancelled)?;
        let next = tokio::select! {
            () = sleep_until(TokioInstant::from_std(deadline)) => {
                return Err(HostError::DockerTimeout);
            }
            () = sleep(CANCEL_POLL) => None,
            item = output.next() => Some(item),
        };
        let Some(next) = next else {
            continue;
        };
        match next {
            None => {
                check_capture_state(started, deadline, cancelled)?;
                return Ok(archive);
            }
            Some(Err(_)) => return Err(HostError::Docker),
            Some(Ok(LogOutput::StdOut { message })) => {
                if archive.len().saturating_add(message.len()) > MAX_ARCHIVE_BYTES {
                    return Err(HostError::Ownership);
                }
                archive.extend_from_slice(&message);
            }
            Some(Ok(_)) => return Err(HostError::Ownership),
        }
    }
}

pub(in crate::launch) fn validate_exec_exit(
    exec_id: &str,
    result: &ExecInspectResponse,
    archive: Vec<u8>,
) -> Result<Vec<u8>, HostError> {
    validate_exec_id(exec_id)?;
    if result.id.as_deref() != Some(exec_id) || result.running != Some(false) {
        return Err(HostError::Docker);
    }
    match result.exit_code {
        Some(0) if !archive.is_empty() && archive.len() <= MAX_ARCHIVE_BYTES => Ok(archive),
        Some(3) if archive.is_empty() => empty_root_archive(),
        _ => Err(HostError::Docker),
    }
}

pub(in crate::launch) fn validate_exec_id(exec_id: &str) -> Result<(), HostError> {
    super::valid_container_id(exec_id)
        .then_some(())
        .ok_or(HostError::Ownership)
}

fn empty_root_archive() -> Result<Vec<u8>, HostError> {
    let mut builder = Builder::new(Vec::new());
    let mut header = Header::new_ustar();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o700);
    header.set_uid(1000);
    header.set_gid(1000);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, ".", std::io::empty())
        .map_err(|_| HostError::Journal)?;
    builder.into_inner().map_err(|_| HostError::Journal)
}

#[cfg(test)]
#[path = "collect_tests.rs"]
mod tests;
