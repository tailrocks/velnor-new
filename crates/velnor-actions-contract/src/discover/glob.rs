//! Exclusion-glob validation and matching for the file index.

use super::index::IndexError;

/// Validate one exclusion glob (relative, no traversal, well-formed).
///
/// # Errors
///
/// Rejects bad patterns with [`IndexError::MalformedPattern`].
pub fn validate_pattern(pattern: &str) -> Result<(), IndexError> {
    let malformed = pattern.is_empty()
        || pattern.starts_with('/')
        || pattern.contains('\\')
        || pattern.split('/').any(|segment| segment == "..")
        || !pattern.bytes().all(is_glob_byte);
    if malformed {
        return Err(IndexError::MalformedPattern(pattern.to_owned()));
    }
    Ok(())
}

/// Bytes allowed in exclusion globs.
fn is_glob_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"/.-_*?[]{}!+@".contains(&byte)
}

/// Whether `path` or an ancestor directory matches any pattern.
#[must_use]
pub fn is_excluded(path: &str, patterns: &[&str]) -> bool {
    patterns
        .iter()
        .any(|pattern| matches_path_or_ancestor(pattern, path))
}

/// Match `path` plus each ancestor prefix against one pattern.
fn matches_path_or_ancestor(pattern: &str, path: &str) -> bool {
    if matches_glob(pattern, path) {
        return true;
    }
    let mut prefix = path;
    while let Some((parent, _)) = prefix.rsplit_once('/') {
        if matches_glob(pattern, parent) {
            return true;
        }
        prefix = parent;
    }
    false
}

/// Match a repository-relative path against one glob (`**`, `*`, `?`, `[...]`, `{a,b}`).
#[must_use]
pub fn matches_glob(pattern: &str, path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    expand_braces(pattern)
        .iter()
        .any(|expanded| match_segments(&expanded.split('/').collect::<Vec<_>>(), &segments))
}

/// Expand the first top-level `{a,b}` group; without braces returns `pattern`.
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
    let (head, _) = pattern.split_at(open);
    let tail = &pattern[close + 1..];
    let mut out = Vec::new();
    for option in inner.split(',') {
        out.extend(expand_braces(&format!("{head}{option}{tail}")));
    }
    out
}

/// Match pattern segments against path segments; `**` spans segments.
fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((head, tail)) if *head == "**" => {
            (0..=path.len()).any(|skip| match_segments(tail, &path[skip..]))
        }
        Some((head, tail)) => path
            .split_first()
            .is_some_and(|(name, rest)| match_segment(head, name) && match_segments(tail, rest)),
    }
}

/// Match one segment with `*`, `?`, and `[...]` classes.
fn match_segment(pattern: &str, name: &str) -> bool {
    match_chars(
        &pattern.chars().collect::<Vec<_>>(),
        &name.chars().collect::<Vec<_>>(),
    )
}

/// Match character patterns against character text.
fn match_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| match_chars(rest, &text[skip..])),
        Some(('?', rest)) => text
            .split_first()
            .is_some_and(|(_, tail)| match_chars(rest, tail)),
        Some(('[', _)) => match_class(pattern, text),
        Some((literal, rest)) => text
            .split_first()
            .is_some_and(|(head, tail)| head == literal && match_chars(rest, tail)),
    }
}

/// Match a leading `[...]` class against the first text character.
fn match_class(pattern: &[char], text: &[char]) -> bool {
    let Some(head) = text.first() else {
        return false;
    };
    let Some(end) = pattern.iter().position(|char| *char == ']') else {
        return false;
    };
    if end < 2 {
        return false;
    }
    let (mut items, rest) = (&pattern[1..end], &pattern[end + 1..]);
    let mut negated = false;
    if items.first() == Some(&'!') {
        negated = true;
        items = &items[1..];
    }
    (class_hit(items, *head) != negated) && match_chars(rest, &text[1..])
}

/// Whether `target` is listed by class `items`, honoring `a-z` ranges.
fn class_hit(items: &[char], target: char) -> bool {
    let mut index = 0;
    while index < items.len() {
        if index + 2 < items.len() && items[index + 1] == '-' {
            if items[index] <= target && target <= items[index + 2] {
                return true;
            }
            index += 3;
        } else {
            if items[index] == target {
                return true;
            }
            index += 1;
        }
    }
    false
}
