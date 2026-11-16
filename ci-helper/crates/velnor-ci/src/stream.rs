// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Error, ErrorKind, Read, Write};
use std::sync::Mutex;

use crate::setup::DoctestCount;
use crate::tests_run::SummaryTap;

const CHUNK: usize = 8192;

#[derive(Debug, Default)]
pub struct Pumped {
    pub failure: Option<Error>,
    pub tests_run: Option<u64>,
}

pub fn pump<L: Write>(from: impl Read, to: impl Write, log: &Mutex<L>) -> Option<Error> {
    pump_counted(from, to, log, None).failure
}

pub fn pump_counted<L: Write>(
    mut from: impl Read,
    mut to: impl Write,
    log: &Mutex<L>,
    count: Option<&Mutex<DoctestCount>>,
) -> Pumped {
    let mut tap = SummaryTap::default();
    let mut buffer = [0_u8; CHUNK];
    let mut failed = None;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                failed = failed.or(Some(error));
                break;
            }
        };
        let chunk = &buffer[..read];
        tap.feed(chunk);
        to.write_all(chunk).and_then(|()| to.flush()).ok();
        if let Ok(mut file) = log.lock()
            && let Err(error) = file.write_all(chunk)
        {
            failed.get_or_insert(error);
        }
        if let Some(Ok(mut seen)) = count.map(Mutex::lock) {
            seen.feed(chunk);
        }
    }
    Pumped {
        failure: failed,
        tests_run: tap.finish(),
    }
}

#[must_use]
pub const fn separate_group(has_terminal: bool) -> bool {
    !has_terminal
}
