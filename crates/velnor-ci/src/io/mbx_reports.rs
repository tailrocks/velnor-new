// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::mbx_report::{DIRECTORY, SWITCH, VARIABLE, parse, reports_session};
use crate::waste_evidence::MbxSession;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const EXPECTED: &str = "expected";
const MAX_ENTRIES: usize = 512;
const MAX_REPORT_BYTES: u64 = 1 << 20;

static SESSIONS: AtomicU64 = AtomicU64::new(0);

fn enabled() -> bool {
    std::env::var(SWITCH).is_ok_and(|value| value == "true")
}

pub(super) fn assign(command: &mut Command, argv: &[String]) {
    command.env_remove(VARIABLE);
    if !enabled() || !reports_session(argv) {
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

pub(super) struct Reports {
    pub(super) sessions: Vec<MbxSession>,
    pub(super) unreadable: Vec<String>,
    pub(super) missing: Vec<String>,
}

pub(super) fn read(runner_temp: &Path) -> Option<Reports> {
    if !enabled() {
        return None;
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(runner_temp.join(DIRECTORY))
        .ok()?
        .flatten()
        .take(MAX_ENTRIES + 1)
        .map(|entry| entry.path())
        .collect();
    let flooded = paths.len() > MAX_ENTRIES;
    paths.truncate(MAX_ENTRIES);
    let with = |wanted: &str| {
        let mut found: Vec<PathBuf> = paths
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == wanted)
            })
            .cloned()
            .collect();
        found.sort();
        found
    };
    let (files, expected) = (with("json"), with(EXPECTED));
    if files.is_empty() && expected.is_empty() && !flooded {
        return None;
    }
    let mut reports = Reports {
        sessions: Vec::new(),
        unreadable: Vec::new(),
        missing: expected
            .iter()
            .filter(|marker| !marker.with_extension("json").exists())
            .map(|marker| {
                marker
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
    };
    if flooded {
        reports.unreadable.push(format!(
            "{DIRECTORY}: more than {MAX_ENTRIES} entries; the rest were not read"
        ));
    }
    for file in files {
        match read_regular(&file).and_then(|bytes| parse(&bytes)) {
            Ok(session) => reports.sessions.extend(session),
            Err(reason) => reports.unreadable.push(format!(
                "{}: {reason}",
                file.file_name().unwrap_or_default().to_string_lossy()
            )),
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
