// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::mbx_report::{DIRECTORY, SWITCH, VARIABLE, parse, reports_session};
use crate::waste_evidence::MbxSession;
use nix::errno::Errno;
use nix::sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, socket};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests;

const EXPECTED: &str = "expected";
const UNRECORDED: &str = "velnor-mbx-reports.unrecorded";
pub(super) const MAX_SESSIONS: usize = 4096;
pub(super) const MAX_ENTRIES: usize = 2 * MAX_SESSIONS;
const MAX_REPORT_BYTES: u64 = 1 << 20;

static SESSIONS: AtomicU64 = AtomicU64::new(0);

fn enabled() -> bool {
    std::env::var(SWITCH).is_ok_and(|value| value == "true")
}

fn inside_session(socket: Option<&OsStr>) -> bool {
    socket.is_some_and(|socket| !socket.is_empty() && is_live_socket(Path::new(socket)))
}

fn is_live_socket(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
        && accepts_without_blocking(path)
}

fn accepts_without_blocking(path: &Path) -> bool {
    let Ok(address) = UnixAddr::new(path) else {
        return false;
    };
    let Ok(endpoint) = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    ) else {
        return false;
    };
    let endpoint = UnixStream::from(endpoint);
    if endpoint.set_nonblocking(true).is_err() {
        return false;
    }
    matches!(
        connect(endpoint.as_raw_fd(), &address),
        Ok(()) | Err(Errno::EAGAIN | Errno::EINPROGRESS)
    )
}

pub(crate) fn assign(command: &mut Command, argv: &[String]) {
    command.env_remove(VARIABLE);
    if !enabled() {
        return;
    }
    let socket = std::env::var_os("MBX_SOCKET");
    let nested = inside_session(socket.as_deref());
    if !nested {
        command.env_remove("MBX_SOCKET");
    }
    if nested || !reports_session(argv) {
        return;
    }
    let Some(temp) = std::env::var_os("RUNNER_TEMP").filter(|temp| !temp.is_empty()) else {
        return;
    };
    let temp = PathBuf::from(temp);
    match expect(&temp) {
        Ok(report) => {
            command.env(VARIABLE, report);
        }
        Err(error) => {
            eprintln!("velnor-ci: {error}; the command runs without an MBX report");
            note_unrecorded(&temp);
        }
    }
}

fn expect(temp: &Path) -> Result<PathBuf, String> {
    let directory = temp.join(DIRECTORY);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let session = SESSIONS.fetch_add(1, Ordering::Relaxed);
    let name = format!("{started:020}-{}-{session}", std::process::id());
    fs::write(directory.join(format!("{name}.{EXPECTED}")), b"")
        .map_err(|error| format!("cannot record the expected MBX report {name}: {error}"))?;
    Ok(directory.join(format!("{name}.json")))
}

fn note_unrecorded(temp: &Path) {
    let file = temp.join(UNRECORDED);
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .and_then(|mut note| note.write_all(b"unrecorded\n"));
    if let Err(error) = written {
        eprintln!(
            "velnor-ci: cannot note an unrecorded MBX session in {}: {error}",
            file.display()
        );
    }
}

pub(super) fn unrecorded(runner_temp: &Path) -> Result<u64, String> {
    if !enabled() {
        return Ok(0);
    }
    let file = runner_temp.join(UNRECORDED);
    if fs::symlink_metadata(&file).is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        return Ok(0);
    }
    let bytes = read_regular(&file)?;
    let text = String::from_utf8(bytes).map_err(|_| "not UTF-8".to_owned())?;
    Ok(u64::try_from(text.lines().count()).unwrap_or(u64::MAX))
}

#[derive(Default)]
pub(super) struct Reports {
    pub(super) sessions: Vec<MbxSession>,
    pub(super) unreadable: Vec<String>,
    pub(super) missing: Vec<String>,
    pub(super) overflowed: bool,
    pub(super) unlisted: usize,
}

#[derive(Default)]
struct Session {
    marker: bool,
    report: Option<PathBuf>,
}

pub(super) fn read(runner_temp: &Path) -> Option<Reports> {
    if !enabled() {
        return None;
    }
    match fs::read_dir(runner_temp.join(DIRECTORY)) {
        Ok(entries) => collect(entries),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => Some(Reports {
            unlisted: 1,
            ..Reports::default()
        }),
    }
}

fn collect(entries: impl Iterator<Item = io::Result<fs::DirEntry>>) -> Option<Reports> {
    let mut listed: Vec<PathBuf> = Vec::new();
    let mut unlisted = 0;
    for entry in entries.take(MAX_ENTRIES + 1) {
        match entry {
            Ok(entry) => listed.push(entry.path()),
            Err(_) => unlisted += 1,
        }
    }
    let mut found: BTreeMap<String, Session> = BTreeMap::new();
    for path in listed.iter().take(MAX_ENTRIES) {
        let Some(name) = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
        else {
            continue;
        };
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(EXPECTED) => found.entry(name).or_default().marker = true,
            Some("json") => found.entry(name).or_default().report = Some(path.clone()),
            _ => {}
        }
    }
    if listed.len() + unlisted > MAX_ENTRIES || found.len() > MAX_SESSIONS {
        return Some(Reports {
            overflowed: true,
            ..Reports::default()
        });
    }
    if found.is_empty() && unlisted == 0 {
        return None;
    }
    let mut reports = Reports {
        unlisted,
        ..Reports::default()
    };
    for (name, session) in found {
        match session.report {
            None if session.marker => reports.missing.push(name),
            None => {}
            Some(file) => match read_regular(&file).and_then(|bytes| {
                if session.marker {
                    parse(&bytes)
                } else {
                    Err("no expected marker".to_owned())
                }
            }) {
                Ok(parsed) => reports.sessions.extend(parsed),
                Err(reason) => reports.unreadable.push(format!(
                    "{}: {reason}",
                    file.file_name().unwrap_or_default().to_string_lossy()
                )),
            },
        }
    }
    Some(reports)
}

fn read_regular(file: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(file).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("not a regular file".to_owned());
    }
    let report = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(file)
        .map_err(|error| error.to_string())?;
    if !report
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("not a regular file".to_owned());
    }
    let mut bytes = Vec::new();
    report
        .take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(format!("larger than {MAX_REPORT_BYTES} bytes"));
    }
    Ok(bytes)
}
