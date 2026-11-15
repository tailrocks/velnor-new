// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Error, ErrorKind, Read, Write};
use std::sync::Mutex;

use crate::setup::DoctestCount;

const CHUNK: usize = 8192;

pub fn pump<L: Write>(from: impl Read, to: impl Write, log: &Mutex<L>) -> Option<Error> {
    pump_counted(from, to, log, None)
}

pub fn pump_counted<L: Write>(
    mut from: impl Read,
    mut to: impl Write,
    log: &Mutex<L>,
    count: Option<&Mutex<DoctestCount>>,
) -> Option<Error> {
    let mut buffer = [0_u8; CHUNK];
    let mut failed = None;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return failed,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return failed.or(Some(error)),
        };
        let chunk = &buffer[..read];
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
}
