// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const JOBS: &str = "jobs:";
const KEY: &str = "cache-mode:";

/// Removes every `cache-mode` key that sits directly under a job of the `jobs` map, because the
/// stand-in runner.server does not model it. Returns the rewritten workflow and the number of keys
/// removed.
#[must_use]
pub fn strip(workflow: &str) -> (String, usize) {
    let mut out = String::with_capacity(workflow.len());
    let mut count = 0;
    let mut in_jobs = false;
    let mut ids: Option<usize> = None;
    let mut body: Option<usize> = None;
    for line in workflow.split_inclusive('\n') {
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            out.push_str(line);
            continue;
        }
        let depth = line.len() - line.trim_start().len();
        if depth == 0 {
            in_jobs = text == JOBS;
            ids = None;
            body = None;
        } else if in_jobs {
            if depth == *ids.get_or_insert(depth) {
                body = None;
            } else if body.is_none() {
                body = Some(depth);
            }
            if body == Some(depth) && text.starts_with(KEY) {
                count += 1;
                continue;
            }
        }
        out.push_str(line);
    }
    (out, count)
}

#[must_use]
pub fn notice(tag: &str, file: &str, count: usize) -> String {
    format!(
        "velnor-local {tag}: dropped {count} job-level cache-mode key(s) from {file}: the stand-in does not model GitHub's cache-mode; cache writes are governed by velnor-local's own cache server"
    )
}
