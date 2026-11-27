// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;
use std::time::Duration;

use velnor_model::RUNNER_PROTOCOL_VERSION;

use super::{api_url, http, token};
use crate::cli_args::RunnerDriftArgs;
use crate::error::CiError;
use crate::runner_drift::{UNKNOWN_EXIT, compare, latest_tag};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);

fn drift(args: &RunnerDriftArgs) -> Result<crate::runner_drift::Drift, CiError> {
    let latest = match &args.latest {
        Some(tag) => tag.clone(),
        None => latest_tag(&http::HttpGitHub::new(api_url(), token()?, LOOKUP_TIMEOUT))?,
    };
    compare(RUNNER_PROTOCOL_VERSION, &latest)
}

pub(super) fn run(args: &RunnerDriftArgs) -> ExitCode {
    match drift(args) {
        Ok(found) => {
            println!("{}", found.line());
            ExitCode::from(u8::from(found.behind()))
        }
        Err(error) => {
            eprintln!("velnor-ci runner-drift: {error}");
            ExitCode::from(UNKNOWN_EXIT)
        }
    }
}
