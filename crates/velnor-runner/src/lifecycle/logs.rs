// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use time::OffsetDateTime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::exec::{JobRun, LogSink, StepResult};

#[derive(Debug)]
pub(super) enum Event {
    StepStarted {
        number: u32,
        name: String,
        at: OffsetDateTime,
    },
    Line {
        number: u32,
        text: String,
        at: OffsetDateTime,
    },
    StepFinished {
        number: u32,
        result: StepResult,
        at: OffsetDateTime,
    },
    JobFinished(Box<JobRun>),
}

#[derive(Debug)]
pub(super) struct GithubSink {
    events: UnboundedSender<Event>,
}

impl GithubSink {
    pub(super) fn channel() -> (Self, UnboundedReceiver<Event>) {
        let (events, receiver) = unbounded_channel();
        (Self { events }, receiver)
    }

    fn send(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

impl LogSink for GithubSink {
    fn step_started(&mut self, number: u32, name: &str) {
        self.send(Event::StepStarted {
            number,
            name: name.to_owned(),
            at: OffsetDateTime::now_utc(),
        });
    }

    fn line(&mut self, number: u32, text: &str) {
        self.send(Event::Line {
            number,
            text: text.to_owned(),
            at: OffsetDateTime::now_utc(),
        });
    }

    fn step_finished(&mut self, number: u32, result: StepResult) {
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
