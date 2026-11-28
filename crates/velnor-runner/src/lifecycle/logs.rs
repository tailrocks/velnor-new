// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::LifecycleError;
use crate::exec::{JobRun, LogSink, StepResult};

pub(super) const STAMP_LEN: usize = "2026-11-27T00:00:00.0000000Z ".len();

#[derive(Debug)]
pub(super) enum Event {
    StepStarted {
        number: u32,
        name: String,
        at: OffsetDateTime,
    },
    StepFinished {
        number: u32,
        result: StepResult,
        at: OffsetDateTime,
    },
    JobFinished(Box<JobRun>),
}

#[derive(Debug, Clone)]
pub(super) struct LogFiles {
    dir: PathBuf,
}

impl LogFiles {
    pub(super) fn dir_of(root: &Path) -> PathBuf {
        root.join("logs")
    }

    pub(super) const fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub(super) fn job_path(&self) -> PathBuf {
        self.dir.join("job.log")
    }

    pub(super) fn step_path(&self, number: u32) -> PathBuf {
        self.dir.join(format!("step-{number}.log"))
    }
}

#[derive(Debug)]
pub(super) struct GithubSink {
    events: UnboundedSender<Event>,
    files: LogFiles,
    job: File,
    steps: BTreeMap<u32, File>,
    write_failed: bool,
}

impl GithubSink {
    pub(super) fn channel(
        files: &LogFiles,
    ) -> Result<(Self, UnboundedReceiver<Event>), LifecycleError> {
        let path = files.job_path();
        let job = File::create(&path).map_err(|source| LifecycleError::WorkDir { path, source })?;
        let (events, receiver) = unbounded_channel();
        let sink = Self {
            events,
            files: files.clone(),
            job,
            steps: BTreeMap::new(),
            write_failed: false,
        };
        Ok((sink, receiver))
    }

    fn send(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn note_failure(&mut self, result: std::io::Result<()>) {
        if let Err(error) = result
            && !self.write_failed
        {
            self.write_failed = true;
            eprintln!("warning: write the job log spill file: {error}");
        }
    }
}

impl LogSink for GithubSink {
    fn step_started(&mut self, number: u32, name: &str) {
        let path = self.files.step_path(number);
        match File::create(&path) {
            Ok(file) => {
                self.steps.insert(number, file);
            }
            Err(error) => self.note_failure(Err(error)),
        }
        self.send(Event::StepStarted {
            number,
            name: name.to_owned(),
            at: OffsetDateTime::now_utc(),
        });
    }

    fn line(&mut self, number: u32, text: &str) {
        let stamped = format!("{}\n", log_line(OffsetDateTime::now_utc(), text));
        let written = self.job.write_all(stamped.as_bytes());
        self.note_failure(written);
        if let Some(file) = self.steps.get_mut(&number) {
            let written = file.write_all(stamped.as_bytes());
            self.note_failure(written);
        }
    }

    fn step_finished(&mut self, number: u32, result: StepResult) {
        self.steps.remove(&number);
        self.send(Event::StepFinished {
            number,
            result,
            at: OffsetDateTime::now_utc(),
        });
    }

    fn job_finished(&mut self, run: &JobRun) {
        self.send(Event::JobFinished(Box::new(run.clone())));
    }
}

pub(super) fn iso_millis(at: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.millisecond()
    )
}

pub(super) fn log_line(at: OffsetDateTime, text: &str) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:07}Z {text}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.nanosecond() / 100
    )
}
