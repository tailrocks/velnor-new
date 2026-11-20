// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::mbx_report::{DIRECTORY, SWITCH, VARIABLE, parse};
use crate::waste_evidence::MbxSession;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SESSIONS: AtomicU64 = AtomicU64::new(0);

fn enabled() -> bool {
    std::env::var(SWITCH).is_ok_and(|value| value == "true")
}

/// Points the command's MBX report at a path no other session of the job uses.
pub(super) fn assign(command: &mut Command) {
    let Some(temp) = std::env::var_os("RUNNER_TEMP").filter(|temp| !temp.is_empty()) else {
        return;
    };
    if !enabled() {
        return;
    }
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
    command.env(
        VARIABLE,
        directory.join(format!(
            "{started:020}-{}-{session}.json",
            std::process::id()
        )),
    );
}

pub(super) struct Reports {
    pub(super) sessions: Vec<MbxSession>,
    pub(super) unreadable: Vec<String>,
}

/// The sessions of every report the job's cargo sessions wrote, in start order; `None` when the
/// job did not ask for reports or no session wrote one.
pub(super) fn read(runner_temp: &Path) -> Option<Reports> {
    if !enabled() {
        return None;
    }
    let mut files: Vec<PathBuf> = fs::read_dir(runner_temp.join(DIRECTORY))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut reports = Reports {
        sessions: Vec::new(),
        unreadable: Vec::new(),
    };
    for file in files {
        match fs::read(&file)
            .map_err(|error| error.to_string())
            .and_then(|bytes| parse(&bytes))
        {
            Ok(session) => reports.sessions.extend(session),
            Err(reason) => reports.unreadable.push(format!(
                "{}: {reason}",
                file.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    Some(reports)
}
