// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{ErrorKind, Read, Write};
use std::sync::Mutex;

const CHUNK: usize = 8192;

pub fn pump<L: Write>(mut from: impl Read, mut to: impl Write, log: &Mutex<L>) {
    let mut buffer = [0_u8; CHUNK];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let chunk = &buffer[..read];
        to.write_all(chunk).and_then(|()| to.flush()).ok();
        if let Ok(mut file) = log.lock() {
            file.write_all(chunk).ok();
        }
    }
}
