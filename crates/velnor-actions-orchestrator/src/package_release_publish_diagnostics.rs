// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use velnor_actions_mise::{
    MiseError, ProcessOutput,
    command::{SPAWN_CANCELLED_MESSAGE, is_cancel_or_timeout, is_cleanup_failure},
};

pub(super) struct RequestDiagnostic {
    operation: &'static str,
    started: Instant,
    timeout: Duration,
    limit: usize,
}

impl RequestDiagnostic {
    pub(super) fn new(args: &[OsString], timeout: Duration, limit: usize) -> Self {
        let route = args
            .iter()
            .filter_map(|arg| arg.to_str())
            .find(|arg| *arg == "graphql" || arg.starts_with("repos/"));
        let operation = match route.map(|route| route.split('?').next().unwrap_or_default()) {
            Some("graphql") => "graphql",
            Some(route) => match route.split('/').skip(3).collect::<Vec<_>>().as_slice() {
                [] => "repository",
                ["releases", "assets", ..] => "release_asset",
                ["releases", ..] => "releases",
                ["actions", "runs", _, "attempts", _, "jobs", ..] => "workflow_jobs",
                ["actions", "runs", ..] => "workflow_runs",
                ["actions", "workflows", ..] => "workflows",
                ["branches", ..] => "branches",
                ["git", ..] => "git_objects",
                ["commits", ..] => "commits",
                ["compare", ..] => "compare",
                ["pulls", ..] => "pull_requests",
                _ => "other",
            },
            None => "other",
        };
        Self {
            operation,
            started: Instant::now(),
            timeout,
            limit,
        }
    }

    pub(super) fn command_error(&self, error: &MiseError) -> OrchestratorError {
        let category = match error {
            MiseError::SpawnFailed { message, .. }
                if message.starts_with("stdout_limit_exceeded:") =>
            {
                "stdout_limit"
            }
            MiseError::SpawnFailed { message, .. }
                if message.starts_with("stderr_limit_exceeded:") =>
            {
                "stderr_limit"
            }
            MiseError::SpawnFailed { message, .. } if is_cancel_or_timeout(error) => {
                if message.split(';').next() == Some(SPAWN_CANCELLED_MESSAGE) {
                    "cancelled"
                } else {
                    "timeout"
                }
            }
            _ => "command",
        };
        self.failure(category, None, None, None, is_cleanup_failure(error))
    }

    pub(super) fn unsuccessful(&self, output: &ProcessOutput) -> OrchestratorError {
        self.failure(
            "nonzero",
            output.code,
            output.signal,
            http_status(&output.stderr),
            false,
        )
    }

    fn failure(
        &self,
        category: &str,
        code: Option<i32>,
        signal: Option<i32>,
        http: Option<u16>,
        cleanup: bool,
    ) -> OrchestratorError {
        internal(&format!(
            "package_publish_api_failed: operation={} category={category} elapsed_ms={} timeout_ms={} cap={} code={} signal={} http={} cleanup_failed={cleanup}",
            self.operation,
            self.started.elapsed().as_millis(),
            self.timeout.as_millis(),
            self.limit,
            code.map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
            signal.map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
            http.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ))
    }
}

fn http_status(stderr: &[u8]) -> Option<u16> {
    let mut observed = None;
    for marker in stderr[..stderr.len().min(8192)].windows(10) {
        if marker.starts_with(b"(HTTP ")
            && marker[9] == b')'
            && marker[6..9].iter().all(u8::is_ascii_digit)
        {
            let value = u16::from(marker[6] - b'0') * 100
                + u16::from(marker[7] - b'0') * 10
                + u16::from(marker[8] - b'0');
            if !(400..=599).contains(&value) || observed.is_some_and(|prior| prior != value) {
                return None;
            }
            observed = Some(value);
        }
    }
    observed
}
