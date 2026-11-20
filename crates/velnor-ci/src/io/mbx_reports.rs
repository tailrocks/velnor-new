// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::mbx_report::{DIRECTORY, SWITCH, VARIABLE, parse, reports_session};
use crate::waste_evidence::MbxSession;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const EXPECTED: &str = "expected";
const MAX_SESSIONS: usize = 4096;
const MAX_ENTRIES: usize = 2 * MAX_SESSIONS;
const MAX_REPORT_BYTES: u64 = 1 << 20;

static SESSIONS: AtomicU64 = AtomicU64::new(0);

fn enabled() -> bool {
    std::env::var(SWITCH).is_ok_and(|value| value == "true")
}

/// An enclosing mbx session (its socket is set) answers for nested cargo calls and writes no report of their own.
fn inside_session() -> bool {
    std::env::var_os("MBX_SOCKET").is_some_and(|socket| !socket.is_empty())
}

pub(super) fn assign(command: &mut Command, argv: &[String]) {
    command.env_remove(VARIABLE);
    if !enabled() || !reports_session(argv) || inside_session() {
        return;
    }
    let Some(temp) = std::env::var_os("RUNNER_TEMP").filter(|temp| !temp.is_empty()) else {
        return;
    };
    let directory = PathBuf::from(temp).join(DIRECTORY);
    if let Err(error) = fs::create_dir_all(&directory) {
        eprintln!(
            "velnor-ci: cannot create {}: {error}; MBX sessions are counted from their stderr summary",
            directory.display()
        );
        return;
    }
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let session = SESSIONS.fetch_add(1, Ordering::Relaxed);
    let name = format!("{started:020}-{}-{session}", std::process::id());
    if let Err(error) = fs::write(directory.join(format!("{name}.{EXPECTED}")), b"") {
        eprintln!(
            "velnor-ci: cannot record the expected MBX report {name}: {error}; the session is counted from its stderr summary"
        );
        return;
    }
    command.env(VARIABLE, directory.join(format!("{name}.json")));
}

#[derive(Default)]
pub(super) struct Reports {
    pub(super) sessions: Vec<MbxSession>,
    pub(super) unreadable: Vec<String>,
    pub(super) missing: Vec<String>,
    pub(super) overflowed: bool,
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
    let listed: Vec<PathBuf> = fs::read_dir(runner_temp.join(DIRECTORY))
        .ok()?
        .flatten()
        .take(MAX_ENTRIES + 1)
        .map(|entry| entry.path())
        .collect();
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
    if listed.len() > MAX_ENTRIES || found.len() > MAX_SESSIONS {
        return Some(Reports {
            overflowed: true,
            ..Reports::default()
        });
    }
    if found.is_empty() {
        return None;
    }
    let mut reports = Reports::default();
    for (name, session) in found {
        match session.report {
            None if session.marker => reports.missing.push(name),
            None => {}
            Some(file) => match read_regular(&file).and_then(|bytes| parse(&bytes)) {
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
    let mut bytes = Vec::new();
    File::open(file)
        .and_then(|report| report.take(MAX_REPORT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(format!("larger than {MAX_REPORT_BYTES} bytes"));
    }
    Ok(bytes)
}
