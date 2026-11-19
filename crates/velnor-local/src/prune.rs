// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::sparse::excluded;

const CHECKOUT: &str = "actions/checkout";
const NAME: &str = "- name: velnor-local prune sparse checkout";
const HEREDOC: &str = "VELNOR_PRUNE";

/// Adds a step after every `actions/checkout` step that sets `sparse-checkout`. The step deletes the
/// tracked paths that this checkout leaves out, so the local workspace matches the sparse tree CI
/// gets. `tracked` lists the paths of the repository at the branch being run. Returns the rewritten
/// workflow and the number of steps added; a workflow that already has the step is left unchanged.
#[must_use]
pub fn prune(workflow: &str, tracked: &[&str]) -> (String, usize) {
    let lines: Vec<&str> = workflow.split_inclusive('\n').collect();
    let mut out = String::with_capacity(workflow.len());
    let mut count = 0;
    let mut at = 0;
    while at < lines.len() {
        let Some(column) = item_column(lines[at]) else {
            out.push_str(lines[at]);
            at += 1;
            continue;
        };
        let end = item_end(&lines, at, column);
        let item = &lines[at..end];
        let Some((patterns, cone)) = checkout_sparse(item) else {
            out.push_str(lines[at]);
            at += 1;
            continue;
        };
        let stop = item
            .iter()
            .rposition(|line| !line.trim().is_empty())
            .map_or(end, |last| at + last + 1);
        out.extend(lines[at..stop].iter().copied());
        if lines.get(end).map(|line| line.trim()) != Some(NAME) {
            out.push_str(&step(column, &excluded(&patterns, cone, tracked)));
            count += 1;
        }
        out.extend(lines[stop..end].iter().copied());
        at = end;
    }
    (out, count)
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn item_column(line: &str) -> Option<usize> {
    let text = line.trim_start();
    (text.starts_with("- ") || text.trim_end() == "-").then(|| indent(line))
}

fn item_end(lines: &[&str], at: usize, column: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(at + 1)
        .find(|(_, line)| !line.trim().is_empty() && indent(line) <= column)
        .map_or(lines.len(), |(end, _)| end)
}

/// The value of `key` on this line, which may be the first key of a list item.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let text = line.trim_start();
    let text = text.strip_prefix("- ").map_or(text, str::trim_start);
    let rest = text.strip_prefix(key)?.strip_prefix(':')?;
    Some(rest.trim())
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix(['\'', '"'])
        .and_then(|inner| inner.strip_suffix(['\'', '"']))
        .unwrap_or(value)
}

/// The sparse patterns and cone mode of a checkout step, or `None` for any other step.
fn checkout_sparse<'a>(item: &[&'a str]) -> Option<(Vec<&'a str>, bool)> {
    let uses = item.iter().find_map(|line| value(line, "uses"))?;
    if !uses.starts_with(CHECKOUT) {
        return None;
    }
    let at = item
        .iter()
        .position(|line| value(line, "sparse-checkout").is_some())?;
    let key = item[at];
    let raw = value(key, "sparse-checkout")?;
    let patterns: Vec<&str> = if raw.starts_with(['|', '>']) {
        item[at + 1..]
            .iter()
            .take_while(|line| line.trim().is_empty() || indent(line) > indent(key))
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect()
    } else {
        vec![unquote(raw)]
    };
    if patterns.is_empty() {
        return None;
    }
    let cone = !item
        .iter()
        .filter_map(|line| value(line, "sparse-checkout-cone-mode"))
        .any(|mode| mode.eq_ignore_ascii_case("false"));
    Some((patterns, cone))
}

fn step(column: usize, paths: &[&str]) -> String {
    let (keys, body) = (" ".repeat(column + 2), " ".repeat(column + 4));
    let head = format!(
        "{}{NAME}\n{keys}shell: bash\n{keys}run: |\n{body}set -euo pipefail\n{body}cd \"$GITHUB_WORKSPACE\"\n{body}while IFS= read -r path; do\n{body}  rm -f -- \"$path\"\n{body}done <<'{HEREDOC}'\n",
        " ".repeat(column)
    );
    let list = paths.iter().fold(String::new(), |mut list, path| {
        list.push_str(&body);
        list.push_str(path);
        list.push('\n');
        list
    });
    format!("{head}{list}{body}{HEREDOC}\n")
}
