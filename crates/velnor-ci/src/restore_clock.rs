// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::error::CiError;
use crate::time::utc_text;

pub const SAFETY_MARGIN_SECONDS: i64 = 300;

const EARLIEST: i64 = 1_577_836_800;
const MAX_RECORD_BYTES: u64 = 64;

const BEGAN: &str = "velnor-tools-restore-began";

pub fn record(runner_temp: &Path, now: i64) -> Result<(), CiError> {
    let path = runner_temp.join(BEGAN);
    let written = match fs::remove_file(&path) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
        _ => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .and_then(|mut file| file.write_all(now.to_string().as_bytes())),
    };
    written.map_err(|error| {
        CiError::new(format!(
            "cannot record when the restore began in {}: {error}",
            path.display()
        ))
    })
}

enum Record {
    Missing,
    Began(i64),
    Untrusted,
}

fn digits(text: &str, now: i64) -> Option<i64> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse()
        .ok()
        .filter(|began| (EARLIEST..=now).contains(began))
}

fn recorded(runner_temp: &Path, now: i64) -> Record {
    let path = runner_temp.join(BEGAN);
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Record::Missing,
        Err(_) => return Record::Untrusted,
    };
    read_capped(file)
        .and_then(|text| digits(&text, now))
        .map_or(Record::Untrusted, Record::Began)
}

fn read_capped(file: fs::File) -> Option<String> {
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_RECORD_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_RECORD_BYTES).then_some(text)
}

#[must_use]
pub fn cutoff(runner_temp: &Path, now: i64) -> Option<String> {
    let began = match recorded(runner_temp, now) {
        Record::Missing => now,
        Record::Began(began) => began,
        Record::Untrusted => return None,
    };
    began.checked_sub(SAFETY_MARGIN_SECONDS).map(utc_text)
}
