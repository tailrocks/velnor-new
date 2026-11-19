// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{EVENT_VARIABLE, api_url, append_outputs, http};
use crate::merge_group::{Context, skip_push};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

fn variable(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

pub(super) fn run() -> String {
    let context = Context {
        event: variable(EVENT_VARIABLE),
        repository: variable("GITHUB_REPOSITORY"),
        sha: variable("GITHUB_SHA"),
        workflow_ref: variable("GITHUB_WORKFLOW_REF"),
        required: Some(variable("VELNOR_REQUIRED_CHECK")).filter(|name| !name.is_empty()),
    };
    let github = http::HttpGitHub::new(api_url(), variable("GITHUB_TOKEN"), LOOKUP_TIMEOUT);
    let result = skip_push(&github, &context);
    let written = append_outputs(&format!("skip={}\n", result.skip));
    match written {
        Ok(()) => format!("{}\n", result.note),
        Err(error) => format!("{}\n(cannot write the output: {error})\n", result.note),
    }
}
