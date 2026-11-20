// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const JOBS: &str = "jobs:";
const KEY: &str = "cache-mode:";

/// Removes every `cache-mode` key that sits directly under a job of the `jobs` map, because the
/// stand-in runner.server does not model it. Returns the rewritten workflow and the number of keys
/// removed. A job or a jobs map written as a flow mapping that carries `cache-mode` is refused,
/// because its key cannot be removed line by line.
pub fn strip(workflow: &str) -> Result<(String, usize), String> {
    let mut out = String::with_capacity(workflow.len());
    let mut count = 0;
    let mut in_jobs = false;
    let mut ids: Option<usize> = None;
    let mut body: Option<usize> = None;
    let mut flow: Option<(String, isize)> = None;
    for line in workflow.split_inclusive('\n') {
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            out.push_str(line);
            continue;
        }
        if let Some((owner, open)) = flow.as_mut() {
            if text.contains("cache-mode") {
                return Err(refusal(owner));
            }
            *open += braces(without_comment(text));
            if *open <= 0 {
                flow = None;
            }
            out.push_str(line);
            continue;
        }
        let code = without_comment(text);
        let depth = line.len() - line.trim_start().len();
        if depth == 0 {
            in_jobs = code == JOBS;
            ids = None;
            body = None;
            if let Some(value) = code.strip_prefix(JOBS).map(str::trim_start)
                && value.starts_with('{')
            {
                enter_flow(&mut flow, "the jobs map".to_owned(), value)?;
            }
        } else if in_jobs {
            if depth == *ids.get_or_insert(depth) {
                body = None;
                if let Some((name, value)) = code.split_once(':')
                    && value.trim_start().starts_with('{')
                {
                    enter_flow(
                        &mut flow,
                        format!("job `{}`", name.trim()),
                        value.trim_start(),
                    )?;
                }
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
    Ok((out, count))
}

fn enter_flow(
    flow: &mut Option<(String, isize)>,
    owner: String,
    value: &str,
) -> Result<(), String> {
    if value.contains("cache-mode") {
        return Err(refusal(&owner));
    }
    let open = braces(value);
    if open > 0 {
        *flow = Some((owner, open));
    }
    Ok(())
}

fn refusal(owner: &str) -> String {
    format!(
        "cannot strip cache-mode from {owner}: it is written as a flow mapping; write it in block style"
    )
}

fn braces(code: &str) -> isize {
    code.matches('{').count().cast_signed() - code.matches('}').count().cast_signed()
}

/// The line without a trailing comment: a `#` that starts a word, as in YAML.
fn without_comment(text: &str) -> &str {
    let mut previous = ' ';
    for (at, c) in text.char_indices() {
        if c == '#' && previous.is_whitespace() {
            return text[..at].trim_end();
        }
        previous = c;
    }
    text
}

#[must_use]
pub fn notice(tag: &str, file: &str, count: usize) -> String {
    format!(
        "velnor-local {tag}: dropped {count} job-level cache-mode key(s) from {file}: the stand-in does not model GitHub's cache-mode; cache writes are governed by velnor-local's own cache server"
    )
}
