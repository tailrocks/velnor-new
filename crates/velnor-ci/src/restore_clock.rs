// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::CiError;
use crate::time::utc_text;

pub const SAFETY_MARGIN_SECONDS: i64 = 300;
pub const ENV_NAME: &str = "VELNOR_TOOLS_RESTORE_BEGAN";

const EARLIEST: i64 = 1_577_836_800;
const MAX_DIGITS: usize = 20;

pub fn record(github_env: &Path, now: i64) -> Result<(), CiError> {
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(github_env)
        .and_then(|mut file| file.write_all(format!("{ENV_NAME}={now}\n").as_bytes()))
        .map_err(|error| {
            CiError::new(format!(
                "cannot record when the restore began in {}: {error}",
                github_env.display()
            ))
        })
}

fn digits(text: &str, now: i64) -> Option<i64> {
    if text.is_empty() || text.len() > MAX_DIGITS || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    text.parse()
        .ok()
        .filter(|began| (EARLIEST..=now).contains(began))
}

#[must_use]
pub fn cutoff(began: Option<&str>, now: i64) -> Option<String> {
    let began = match began {
        None => now,
        Some(text) => digits(text, now)?,
    };
    began.checked_sub(SAFETY_MARGIN_SECONDS).map(utc_text)
}
