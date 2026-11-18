//! Git subprocess and mtime helpers shared by the inputs snapshot code.

use std::fs::{self, File, FileTimes};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::CiError;

pub(super) fn git(root: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, CiError> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CiError::new(format!("cannot run git {}: {e}", args[0])))?;
    let input = child.stdin.take();
    let output = std::thread::scope(|scope| {
        let writer = scope.spawn(move || match (input, stdin) {
            (Some(mut pipe), Some(bytes)) => pipe.write_all(bytes),
            _ => Ok(()),
        });
        let output = child.wait_with_output();
        let written = writer
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("stdin writer panicked")));
        output.and_then(|output| written.map(|()| output))
    })
    .map_err(|e| CiError::new(format!("git {}: {e}", args[0])))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

pub(super) fn modified_ns(path: &Path) -> Result<u64, CiError> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos()).ok())
        .ok_or_else(|| CiError::new(format!("mtime of {} is out of range", path.display())))
}

pub(super) fn set_modified_ns(path: &Path, mtime_ns: u64) -> Result<(), CiError> {
    let time = SystemTime::UNIX_EPOCH + Duration::from_nanos(mtime_ns);
    File::options()
        .write(true)
        .open(path)
        .or_else(|_| File::open(path))
        .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
        .map_err(|e| CiError::new(format!("cannot set mtime of {}: {e}", path.display())))
}
