// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Error, ErrorKind, Read, Write};
use std::sync::Mutex;

use crate::setup::DoctestCount;
use crate::tests_run::{SummaryTap, TestCounts};

const CHUNK: usize = 8192;

#[derive(Debug, Default)]
pub struct Pumped {
    pub failure: Option<Error>,
    pub console: Option<Error>,
}

pub fn pump<L: Write>(from: impl Read, to: impl Write, log: &Mutex<L>) -> Option<Error> {
    pump_counted(from, to, log, None, None, None).failure
}

pub fn pump_counted<L: Write>(
    mut from: impl Read,
    mut to: impl Write,
    log: &Mutex<L>,
    count: Option<&Mutex<DoctestCount>>,
    parsed: Option<&Mutex<Option<TestCounts>>>,
    reported: Option<&Mutex<Pumped>>,
) -> Pumped {
    let mut tap = SummaryTap::default();
    let mut buffer = [0_u8; CHUNK];
    let mut failed = None;
    let mut console = None;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => {
                if let Some(Ok(mut seen)) = count.map(Mutex::lock) {
                    seen.end_stream();
                }
                break;
            }
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                failed = failed.or(Some(error));
                publish(reported, &failed, &console);
                break;
            }
        };
        let chunk = &buffer[..read];
        tap.feed(chunk);
        remember(parsed, tap.counts());
        if console.is_none()
            && let Err(error) = to.write_all(chunk).and_then(|()| to.flush())
        {
            console = Some(error);
            publish(reported, &failed, &console);
        }
        if let Ok(mut file) = log.lock()
            && let Err(error) = file.write_all(chunk)
        {
            failed.get_or_insert(error);
            publish(reported, &failed, &console);
        }
        if let Some(Ok(mut seen)) = count.map(Mutex::lock) {
            seen.feed(chunk);
        }
    }
    remember(parsed, tap.finish());
    Pumped {
        failure: failed,
        console,
    }
}

fn publish(reported: Option<&Mutex<Pumped>>, failure: &Option<Error>, console: &Option<Error>) {
    if let Some(reported) = reported
        && let Ok(mut reported) = reported.lock()
    {
        reported.failure = reported
            .failure
            .take()
            .or_else(|| failure.as_ref().map(copy_error));
        reported.console = reported
            .console
            .take()
            .or_else(|| console.as_ref().map(copy_error));
    }
}

fn copy_error(error: &Error) -> Error {
    Error::new(error.kind(), error.to_string())
}

fn remember(parsed: Option<&Mutex<Option<TestCounts>>>, counts: Option<TestCounts>) {
    if let Some(parsed) = parsed
        && let Some(counts) = counts
    {
        *parsed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(counts);
    }
}

#[must_use]
pub const fn separate_group(has_terminal: bool) -> bool {
    !has_terminal
}
