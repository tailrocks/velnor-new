// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const ANY_DIRECTORY: &str = "*";
const DIRECTORY_TAIL: &str = "/**";

pub(super) fn matches(pattern: &str, path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    expand_braces(pattern)
        .iter()
        .any(|expanded| match_segments(&expanded.split('/').collect::<Vec<_>>(), &segments))
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(close) = pattern[open..].find('}') else {
        return vec![pattern.to_owned()];
    };
    let close = open + close;
    let inner = &pattern[open + 1..close];
    if !inner.contains(',') {
        return vec![pattern.to_owned()];
    }
    let head = &pattern[..open];
    let tail = &pattern[close + 1..];
    inner
        .split(',')
        .flat_map(|option| expand_braces(&format!("{head}{option}{tail}")))
        .collect()
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((head, tail)) if *head == "**" => (0..=path.len()).any(|skip| {
            path.get(skip..)
                .is_some_and(|rest| match_segments(tail, rest))
        }),
        Some((head, tail)) => path
            .split_first()
            .is_some_and(|(name, rest)| match_segment(head, name) && match_segments(tail, rest)),
    }
}

fn match_segment(pattern: &str, name: &str) -> bool {
    match_chars(
        &pattern.chars().collect::<Vec<_>>(),
        &name.chars().collect::<Vec<_>>(),
    )
}

fn match_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len())
            .any(|skip| text.get(skip..).is_some_and(|tail| match_chars(rest, tail))),
        Some(('?', rest)) => text
            .split_first()
            .is_some_and(|(_, tail)| match_chars(rest, tail)),
        Some(('[', _)) => match_class(pattern, text),
        Some((literal, rest)) => text
            .split_first()
            .is_some_and(|(head, tail)| head == literal && match_chars(rest, tail)),
    }
}

fn match_class(pattern: &[char], text: &[char]) -> bool {
    let Some((head, tail)) = text.split_first() else {
        return false;
    };
    let Some(end) = pattern.iter().position(|char| *char == ']') else {
        return false;
    };
    if end < 2 {
        return false;
    }
    let (Some(mut items), Some(rest)) = (pattern.get(1..end), pattern.get(end + 1..)) else {
        return false;
    };
    let mut negated = false;
    if items.first() == Some(&'!') {
        negated = true;
        items = items.get(1..).unwrap_or_default();
    }
    (class_hit(items, *head) != negated) && match_chars(rest, tail)
}

fn class_hit(items: &[char], target: char) -> bool {
    let mut index = 0;
    while let Some(item) = items.get(index) {
        let range_end = items
            .get(index + 1)
            .zip(items.get(index + 2))
            .filter(|(dash, _)| **dash == '-')
            .map(|(_, end)| *end);
        if let Some(end) = range_end {
            if *item <= target && target <= end {
                return true;
            }
            index += 3;
        } else {
            if *item == target {
                return true;
            }
            index += 1;
        }
    }
    false
}

pub(super) fn non_input_covers(glob: &str, path: &str) -> bool {
    let Some(prefix) = directory_prefix(glob) else {
        return false;
    };
    let mut parts = path.split('/');
    prefix.iter().all(|want| {
        parts
            .next()
            .is_some_and(|got| !got.is_empty() && (*want == ANY_DIRECTORY || *want == got))
    }) && parts.next().is_some()
}

fn directory_prefix(glob: &str) -> Option<Vec<&str>> {
    let segments: Vec<&str> = glob.strip_suffix(DIRECTORY_TAIL)?.split('/').collect();
    segments
        .iter()
        .all(|segment| is_segment(segment))
        .then_some(segments)
}

fn is_segment(segment: &str) -> bool {
    segment == ANY_DIRECTORY
        || (segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            && segment.bytes().any(|byte| byte != b'.'))
}
