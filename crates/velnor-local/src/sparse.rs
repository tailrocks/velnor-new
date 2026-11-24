// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

/// The tracked paths that a sparse checkout with these patterns leaves out, in the order given.
/// Cone mode lists directories (`cone` is `sparse-checkout-cone-mode`); otherwise the patterns are
/// gitignore-style, the last matching pattern decides, and a path matched by none is left out.
#[must_use]
pub fn excluded<'a>(patterns: &[&str], cone: bool, tracked: &[&'a str]) -> Vec<&'a str> {
    tracked
        .iter()
        .copied()
        .filter(|path| {
            let kept = if cone {
                cone_keeps(patterns, path)
            } else {
                gitignore_keeps(patterns, path)
            };
            !kept
        })
        .collect()
}

fn under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn cone_keeps(dirs: &[&str], path: &str) -> bool {
    let parent = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    parent.is_empty()
        || dirs.iter().any(|dir| {
            let dir = dir.trim_matches('/');
            dir == parent || under(dir, parent) || under(parent, dir)
        })
}

fn gitignore_keeps(patterns: &[&str], path: &str) -> bool {
    let mut keep = false;
    for line in patterns.iter().map(|line| line.trim()) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negated, body) = line
            .strip_prefix('!')
            .map_or((false, line), |rest| (true, rest));
        if matches_path(body, path) {
            keep = !negated;
        }
    }
    keep
}

fn matches_path(pattern: &str, path: &str) -> bool {
    let dir_only = pattern.ends_with('/');
    let core = pattern.trim_end_matches('/');
    let anchored = core.contains('/');
    let glob = core.trim_start_matches('/').as_bytes();
    let ends = path
        .match_indices('/')
        .map(|(at, _)| at)
        .chain(std::iter::once(path.len()));
    ends.map(|end| (&path[..end], end < path.len()))
        .filter(|(_, is_dir)| !dir_only || *is_dir)
        .any(|(candidate, _)| {
            let subject = if anchored {
                candidate
            } else {
                candidate.rsplit('/').next().unwrap_or(candidate)
            };
            glob_matches(glob, subject.as_bytes())
        })
}

fn glob_matches(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            let (any, rest) = match rest.split_first() {
                Some((b'*', tail)) => (true, tail),
                _ => (false, rest),
            };
            (0..=text.len())
                .any(|at| (any || !text[..at].contains(&b'/')) && glob_matches(rest, &text[at..]))
        }
        Some((b'?', rest)) => {
            text.first().is_some_and(|byte| *byte != b'/') && glob_matches(rest, &text[1..])
        }
        Some((byte, rest)) => text.first() == Some(byte) && glob_matches(rest, &text[1..]),
    }
}
