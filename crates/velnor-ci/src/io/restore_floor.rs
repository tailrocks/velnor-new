// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime};

use crate::error::CiError;

const FLOOR: &str = "velnor-restore-floor";
const ATTEMPTS: u32 = 100;
const STEP: Duration = Duration::from_millis(1);

pub(super) fn git_path(root: &Path, name: &str) -> Result<PathBuf, CiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-path", name])
        .output()
        .map_err(|e| failed("locate", root, e))?;
    if !output.status.success() {
        return Err(failed(
            "locate",
            root,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let path = String::from_utf8(output.stdout).map_err(|e| failed("locate", root, e))?;
    Ok(PathBuf::from(path.trim_end_matches(['\n', '\r'])))
}

pub(super) fn mark(root: &Path) -> Result<String, CiError> {
    let path = git_path(root, FLOOR)?;
    let mut previous = stamp(&path, 0)?;
    for attempt in 1..=ATTEMPTS {
        sleep(STEP);
        let current = stamp(&path, attempt)?;
        if current > previous {
            fs::write(&path, nanoseconds(current).to_string())
                .map_err(|e| failed("write", &path, e))?;
            return Ok(String::new());
        }
        previous = current;
    }
    remove(&path)?;
    Ok(format!(
        "the change time of {} did not advance within {} ms, so no restore floor was recorded and the audit counts every restored input as changed\n",
        path.display(),
        STEP.as_millis() * u128::from(ATTEMPTS)
    ))
}

pub(in crate::io) fn read(root: &Path) -> Option<SystemTime> {
    let text = fs::read_to_string(git_path(root, FLOOR).ok()?).ok()?;
    let nanos = text.trim().parse::<u64>().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_nanos(nanos))
}

fn stamp(path: &Path, attempt: u32) -> Result<SystemTime, CiError> {
    fs::write(path, attempt.to_string()).map_err(|e| failed("write", path, e))?;
    let metadata = fs::metadata(path).map_err(|e| failed("read", path, e))?;
    let seconds = u64::try_from(metadata.ctime()).map_err(|e| failed("read", path, e))?;
    let nanos = u32::try_from(metadata.ctime_nsec()).map_err(|e| failed("read", path, e))?;
    Ok(SystemTime::UNIX_EPOCH + Duration::new(seconds, nanos))
}

fn nanoseconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos()).ok())
        .unwrap_or(0)
}

fn remove(path: &Path) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failed("remove", path, e)),
    }
}

fn failed(what: &str, path: &Path, error: impl std::fmt::Display) -> CiError {
    CiError::new(format!(
        "cannot {what} the restore floor {}: {error}",
        path.display()
    ))
}
