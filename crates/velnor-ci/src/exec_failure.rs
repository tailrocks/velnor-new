// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const MARKER: &str = "[double-spawn] failed to exec \"";
const DENIED: &str = "Permission denied (os error 13)";
const LIMIT: usize = 3;

#[must_use]
pub(crate) fn denied_execs(text: &str) -> Vec<&str> {
    let mut paths: Vec<&str> = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(MARKER) else {
            continue;
        };
        let Some((path, tail)) = rest.split_once('"') else {
            continue;
        };
        if tail.contains(DENIED) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.truncate(LIMIT);
    paths
}
