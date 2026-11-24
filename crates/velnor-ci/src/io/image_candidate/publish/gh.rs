// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::super::process::Runner;
use crate::error::CiError;
use crate::image_candidate::json::parse;
use crate::image_publish::checks::{
    CiRun, EMPTY_LISTING_RETRY_SECONDS, ci_listing, ci_path, ci_run,
};

const API_TIMEOUT: Duration = Duration::from_secs(30);
const ATTESTATION_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: u64 = 2 * 1024 * 1024;

pub(super) struct Gh<'a> {
    pub(super) runner: &'a Runner,
    pub(super) program: &'a Path,
}

impl Gh<'_> {
    fn json(&self, args: &[String], timeout: Duration, label: &str) -> Result<Value, CiError> {
        let mut command = Command::new(self.program);
        command.args(args).env_remove("GH_HOST");
        let output = self.runner.text_up_to(&mut command, timeout, MAX_OUTPUT);
        self.runner.checkpoint()?;
        let text = output.map_err(|_| CiError::new("GitHub CLI request failed."))?;
        parse(text.as_bytes(), label)
    }

    pub(super) fn api(&self, path: &str) -> Result<Value, CiError> {
        self.json(
            &["api".to_owned(), path.to_owned()],
            API_TIMEOUT,
            "GitHub API response",
        )
    }

    pub(super) fn attestation(&self, args: &[String]) -> Result<Value, CiError> {
        self.json(args, ATTESTATION_TIMEOUT, "Attestation verification output")
    }

    pub(super) fn ci(&self, source: &str, pause: &dyn Fn(Duration)) -> Result<CiRun, CiError> {
        let path = ci_path(source);
        let mut delays = EMPTY_LISTING_RETRY_SECONDS.iter();
        loop {
            let response = self.api(&path)?;
            let runs = ci_listing(&response)?;
            match delays.next() {
                Some(seconds) if runs.is_empty() => pause(Duration::from_secs(*seconds)),
                _ => return ci_run(runs, source),
            }
        }
    }
}
