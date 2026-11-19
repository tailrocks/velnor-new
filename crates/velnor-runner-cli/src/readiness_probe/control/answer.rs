// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant as StdInstant};

use velnor_runner_host::{CreatedSetDeletion, HostError, RepositoryConnection};

use super::{ControlAnswer, control_job, run_answer_job};

pub(super) const MAX_ANSWER_BYTES: usize = 12;

impl ControlAnswer {
    pub(super) fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
        match text {
            "true" => Some(Self::Flag(true)),
            "false" => Some(Self::Flag(false)),
            "deleted" => Some(Self::Deletion(CreatedSetDeletion::Deleted)),
            "not_owned" => Some(Self::Deletion(CreatedSetDeletion::NotOwned)),
            "uncertain" => Some(Self::Deletion(CreatedSetDeletion::Uncertain)),
            "timed_out" => Some(Self::Deletion(CreatedSetDeletion::TimedOut)),
            "offline" => Some(Self::Deletion(CreatedSetDeletion::ControllerOffline)),
            other => {
                let status = other.strip_prefix("refused ")?;
                Some(Self::Deletion(CreatedSetDeletion::Refused(
                    status.parse().ok()?,
                )))
            }
        }
    }

    fn line(self) -> String {
        match self {
            Self::Flag(true) => "true\n".to_owned(),
            Self::Flag(false) => "false\n".to_owned(),
            Self::Deletion(CreatedSetDeletion::Deleted) => "deleted\n".to_owned(),
            Self::Deletion(CreatedSetDeletion::NotOwned) => "not_owned\n".to_owned(),
            Self::Deletion(CreatedSetDeletion::Uncertain) => "uncertain\n".to_owned(),
            Self::Deletion(CreatedSetDeletion::TimedOut) => "timed_out\n".to_owned(),
            Self::Deletion(CreatedSetDeletion::ControllerOffline) => "offline\n".to_owned(),
            Self::Deletion(CreatedSetDeletion::Refused(status)) => format!("refused {status}\n"),
        }
    }
}

pub(super) fn write_answer(answer: ControlAnswer) -> ExitCode {
    let mut output = std::io::stdout().lock();
    if output.write_all(answer.line().as_bytes()).is_err() || output.flush().is_err() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

pub(crate) fn delete_created_scale_set_until(
    state: &Path,
    binding: &RepositoryConnection,
    deadline: StdInstant,
) -> Result<CreatedSetDeletion, HostError> {
    let binding = serde_json::to_value(binding).map_err(|_| HostError::Config)?;
    let request = control_job("delete_set", state, None, Some(binding), deadline)?;
    match run_answer_job(request, deadline)? {
        Some(ControlAnswer::Deletion(deletion)) => Ok(deletion),
        Some(ControlAnswer::Flag(false)) | None => Ok(CreatedSetDeletion::TimedOut),
        Some(ControlAnswer::Flag(true)) => Err(HostError::Lock),
    }
}

pub(super) fn run_delete_leaf(
    state: &Path,
    request: &serde_json::Value,
    timeout: Duration,
) -> ExitCode {
    let Some(value) = request.get("binding").cloned() else {
        return ExitCode::from(2);
    };
    let Ok(binding) = serde_json::from_value::<RepositoryConnection>(value) else {
        return ExitCode::from(2);
    };
    match velnor_runner_host::delete_created_scale_set(state, &binding, timeout) {
        Ok(deletion) => write_answer(ControlAnswer::Deletion(deletion)),
        Err(_) => ExitCode::from(1),
    }
}

#[cfg(test)]
mod tests {
    use velnor_runner_host::CreatedSetDeletion;

    use super::{ControlAnswer, MAX_ANSWER_BYTES};

    #[test]
    fn every_answer_survives_the_trip_between_control_processes() {
        let answers = [
            ControlAnswer::Flag(true),
            ControlAnswer::Flag(false),
            ControlAnswer::Deletion(CreatedSetDeletion::Deleted),
            ControlAnswer::Deletion(CreatedSetDeletion::NotOwned),
            ControlAnswer::Deletion(CreatedSetDeletion::Refused(422)),
            ControlAnswer::Deletion(CreatedSetDeletion::Uncertain),
            ControlAnswer::Deletion(CreatedSetDeletion::TimedOut),
            ControlAnswer::Deletion(CreatedSetDeletion::ControllerOffline),
        ];
        for answer in answers {
            let line = answer.line();
            assert!(line.len() <= MAX_ANSWER_BYTES, "{line:?}");
            assert_eq!(ControlAnswer::parse(line.as_bytes()), Some(answer));
        }
        assert_eq!(ControlAnswer::parse(b"refused 99999\n"), None);
        assert_eq!(ControlAnswer::parse(b"deleted"), None);
    }
}
