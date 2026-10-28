// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::super::CollectorError;
use velnor_actions_mise::{BoundedCommandOutput, BoundedCommandStatus, BoundedCommandTermination};

pub(super) fn persist_result(
    raw: &Path,
    name: &str,
    artifact: &Path,
    output: &BoundedCommandOutput,
) -> Result<(), CollectorError> {
    let mut errors = Vec::new();
    write(
        raw.join(format!("{name}.raw.txt")),
        &output.stdout,
        &mut errors,
    );
    write(
        raw.join(format!("{name}.stderr.raw.txt")),
        &output.stderr,
        &mut errors,
    );
    write_text(
        raw.join(format!("{name}.status")),
        &format!("{}\n", status_text(output.status)),
        &mut errors,
    );
    write_text(
        raw.join(format!("{name}.termination")),
        &format!("{}\n", termination_text(output.termination)),
        &mut errors,
    );
    write_text(
        artifact.join(format!("{name}.exit")),
        &format!("{}\n", status_text(output.status)),
        &mut errors,
    );
    if let Some(error) = &output.execution_error {
        write_text(
            raw.join(format!("{name}.execution-error")),
            &single_line(error),
            &mut errors,
        );
    }
    if let Some(error) = &output.cleanup_error {
        write_text(
            raw.join(format!("{name}.cleanup-error")),
            &single_line(error),
            &mut errors,
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CollectorError::operational(errors.join("; ")))
    }
}

fn write(path: impl AsRef<Path>, bytes: &[u8], errors: &mut Vec<String>) {
    if let Err(error) = fs::write(path.as_ref(), bytes) {
        errors.push(format!("{}: {error}", path.as_ref().display()));
    }
}

fn write_text(path: impl AsRef<Path>, value: &str, errors: &mut Vec<String>) {
    write(path, value.as_bytes(), errors);
}

fn status_text(status: BoundedCommandStatus) -> String {
    match status {
        BoundedCommandStatus::Exited(code) => code.to_string(),
        BoundedCommandStatus::Signaled(signal) => format!("signal:{signal}"),
        BoundedCommandStatus::NotStarted => "not-started".to_owned(),
        BoundedCommandStatus::Unavailable => "unavailable".to_owned(),
    }
}

fn termination_text(termination: BoundedCommandTermination) -> &'static str {
    match termination {
        BoundedCommandTermination::Completed => "completed",
        BoundedCommandTermination::TimedOut => "timed-out",
        BoundedCommandTermination::OutputLimitExceeded => "output-limit-exceeded",
        BoundedCommandTermination::Cancelled => "cancelled",
        BoundedCommandTermination::SpawnFailed => "spawn-failed",
        BoundedCommandTermination::ExecutionFailed => "execution-failed",
        BoundedCommandTermination::CleanupFailed => "cleanup-failed",
    }
}

fn single_line(value: &str) -> String {
    format!("{}\n", value.replace(['\n', '\r'], " "))
}
