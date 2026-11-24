// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{EVENT_VARIABLE, api_url, http};
use crate::merge_queue::{Context, queue_warning};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

fn variable(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

pub(super) fn run() -> String {
    let context = Context {
        event: variable(EVENT_VARIABLE),
        repository: variable("GITHUB_REPOSITORY"),
        base: variable("GITHUB_BASE_REF"),
    };
    let github = http::HttpGitHub::new(api_url(), variable("GITHUB_TOKEN"), LOOKUP_TIMEOUT);
    queue_warning(&github, &context).map_or_else(String::new, |line| format!("{line}\n"))
}
