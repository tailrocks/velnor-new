// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;

use super::{JobRun, StepResult};

pub trait LogSink: Send {
    fn step_started(&mut self, number: u32, name: &str);
    fn line(&mut self, number: u32, text: &str);
    fn step_finished(&mut self, number: u32, result: StepResult);
    fn job_finished(&mut self, run: &JobRun);
}

#[derive(Debug, Default)]
pub struct StdoutSink;

fn emit(text: &str) {
    let _ = writeln!(std::io::stdout().lock(), "{text}");
}

impl LogSink for StdoutSink {
    fn step_started(&mut self, number: u32, name: &str) {
        emit(&format!("step {number} start: {name}"));
    }

    fn line(&mut self, _number: u32, text: &str) {
        emit(text);
    }

    fn step_finished(&mut self, number: u32, result: StepResult) {
        emit(&format!("step {number} {}", result.label()));
    }

    fn job_finished(&mut self, run: &JobRun) {
        for (key, value) in &run.outputs {
            emit(&format!("output {key}={value}"));
        }
        emit(&format!("job {}", run.result.label()));
    }
}
