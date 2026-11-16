// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Error, ErrorKind, Read, Write};
use std::sync::Mutex;

use crate::cargo_artifacts::{ArtifactBatch, ArtifactTap};
use crate::setup::DoctestCount;
use crate::tests_run::SummaryTap;

const CHUNK: usize = 8192;

#[derive(Debug, Default)]
pub(super) struct Capture {
    pub failure: Option<Error>,
    pub tests_run: Option<u64>,
    pub observations: ArtifactBatch,
}

impl Capture {
    fn publish(&mut self, batch: ArtifactBatch, tests_run: Option<u64>) {
        self.observations.artifacts.extend(batch.artifacts);
        self.observations.progress.extend(batch.progress);
        self.observations.tests.extend(batch.tests);
        self.observations.rejected += batch.rejected;
        self.tests_run = tests_run.or(self.tests_run);
    }
}

pub fn pump<L: Write>(from: impl Read, to: impl Write, log: &Mutex<L>) -> Option<Error> {
    let capture = Mutex::new(Capture::default());
    pump_counted(from, to, log, None, &capture);
    capture
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .failure
}

pub(super) fn pump_counted<L: Write>(
    mut from: impl Read,
    mut to: impl Write,
    log: &Mutex<L>,
    count: Option<&Mutex<DoctestCount>>,
    capture: &Mutex<Capture>,
) -> bool {
    let mut tap = SummaryTap::default();
    let mut artifacts = ArtifactTap::default();
    let mut buffer = [0_u8; CHUNK];
    let complete = loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break true,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .failure
                    .get_or_insert(error);
                break false;
            }
        };
        let chunk = &buffer[..read];
        tap.feed(chunk);
        artifacts.feed(chunk);
        capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publish(artifacts.take_ready(), tap.tests_run());
        to.write_all(chunk).and_then(|()| to.flush()).ok();
        let failure = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write_all(chunk)
            .err();
        if let Some(error) = failure {
            capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .failure
                .get_or_insert(error);
        }
        if let Some(Ok(mut seen)) = count.map(Mutex::lock) {
            seen.feed(chunk);
        }
    };
    let batch = if complete {
        artifacts.finish()
    } else {
        artifacts.take_ready()
    };
    let tests_run = if complete {
        tap.finish()
    } else {
        tap.tests_run()
    };
    capture
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .publish(batch, tests_run);
    complete
}
