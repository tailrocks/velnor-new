//! A deliberately tiny reader for the YAML subset dbt control files use.
//!
//! It understands top-level `key: scalar` lines, flow lists (`["a", "b"]`),
//! block lists (`- item`) under a top-level key, and one level of list-of-map
//! entries (`- key: value` followed by deeper `key: value` lines). Anything it
//! cannot classify is reported, never guessed.

use std::collections::BTreeMap;

/// Strip a trailing comment that starts a token (` #`) outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_space = true;
    for (index, ch) in line.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '#' && prev_space => return &line[..index],
            _ => {}
        }
        prev_space = ch.is_whitespace();
    }
    line
}

/// Unquote one scalar.
#[must_use]
pub fn scalar(raw: &str) -> String {
    let raw = raw.trim();
    for q in ['"', '\''] {
        if let Some(inner) = raw.strip_prefix(q).and_then(|rest| rest.strip_suffix(q)) {
            return inner.to_owned();
        }
    }
    raw.to_owned()
}

/// Items of a flow list `[a, "b"]`, or `None` when `raw` is not one.
#[must_use]
pub fn flow_list(raw: &str) -> Option<Vec<String>> {
    let inner = raw.trim().strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(scalar)
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

/// Logical lines: `(indent, text)` with comments and blank lines removed.
fn lines(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .filter_map(|line| {
            let line = strip_comment(line).trim_end();
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed == "---" {
                return None;
            }
            Some((line.len() - trimmed.len(), trimmed.to_owned()))
        })
        .collect()
}

fn split_pair(text: &str) -> Option<(String, String)> {
    let (key, value) = text.split_once(':')?;
    if key.is_empty() || key.contains(' ') && !key.starts_with('"') {
        return None;
    }
    Some((scalar(key), value.trim().to_owned()))
}

/// Top-level scalar keys of a document (`key: value` at indent 0, value non-empty).
#[must_use]
pub fn top_scalars(text: &str) -> BTreeMap<String, String> {
    lines(text)
        .into_iter()
        .filter(|(indent, _)| *indent == 0)
        .filter_map(|(_, line)| split_pair(&line))
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key, scalar(&value)))
        .collect()
}

/// Items of a top-level list key (flow or block form of plain scalars).
#[must_use]
pub fn top_list(text: &str, key: &str) -> Option<Vec<String>> {
    let all = lines(text);
    let position = all.iter().position(|(indent, line)| {
        *indent == 0 && split_pair(line).is_some_and(|(k, _)| k == key)
    })?;
    let (_, value) = split_pair(&all[position].1)?;
    if let Some(items) = flow_list(&value) {
        return Some(items);
    }
    let mut items = Vec::new();
    for (indent, line) in &all[position + 1..] {
        if let Some(item) = line.strip_prefix("- ") {
            items.push(scalar(item));
        } else if *indent == 0 {
            break;
        }
    }
    Some(items)
}

/// Entries of a top-level list-of-maps key: each entry is its `key: value` pairs.
///
/// `Err` carries the offending line when a line under the key is neither a
/// `- key: value` entry start nor a deeper `key: value` continuation.
///
/// # Errors
///
/// The first unclassifiable line.
pub fn top_entries(text: &str, key: &str) -> Result<Option<Vec<BTreeMap<String, String>>>, String> {
    let all = lines(text);
    let Some(position) = all
        .iter()
        .position(|(indent, line)| *indent == 0 && split_pair(line).is_some_and(|(k, _)| k == key))
    else {
        return Ok(None);
    };
    let mut entries: Vec<BTreeMap<String, String>> = Vec::new();
    let mut dash_indent = None;
    for (indent, line) in &all[position + 1..] {
        if *indent == 0 && !line.starts_with("- ") {
            break;
        }
        if let Some(rest) = line.strip_prefix("- ") {
            dash_indent = Some(*indent);
            let (k, v) = split_pair(rest).ok_or_else(|| line.clone())?;
            entries.push(BTreeMap::from([(k, scalar(&v))]));
        } else if dash_indent.is_some_and(|dash| *indent > dash) {
            let (k, v) = split_pair(line).ok_or_else(|| line.clone())?;
            let entry = entries.last_mut().ok_or_else(|| line.clone())?;
            entry.insert(k, scalar(&v));
        } else {
            return Err(line.clone());
        }
    }
    Ok(Some(entries))
}
