// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_rust::MetadataError;

use super::SAMPLE_REPOSITORY;
use crate::io::Error;

pub fn relocate_metadata(checkout: &Path, json: &[u8]) -> Result<Vec<u8>, Error> {
    let checkout = checkout
        .to_str()
        .ok_or_else(|| Error::Repository("the checkout path is not UTF-8".to_owned()))?;
    let original = checkout;
    let unc = checkout
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"));
    let checkout = unc
        .as_deref()
        .unwrap_or_else(|| checkout.strip_prefix(r"\\?\").unwrap_or(checkout));
    let from = checkout.trim_end_matches(['/', '\\']);
    let is_drive_root = from.len() == 2
        && from.as_bytes()[0].is_ascii_alphabetic()
        && from.as_bytes()[1] == b':'
        && from.len() < checkout.len();
    if from.is_empty() || is_drive_root {
        return Err(Error::Repository(
            "a checkout at the filesystem root cannot be recorded".to_owned(),
        ));
    }
    let mut value: serde_json::Value = serde_json::from_slice(json)
        .map_err(|e| Error::Metadata(MetadataError::Invalid(e.to_string())))?;
    let normalized = from.replace('\\', "/");
    let encoded = percent_encode(from);
    let encoded_normalized = percent_encode(&normalized);
    let mut prefixes = vec![
        from.to_owned(),
        normalized.clone(),
        encoded,
        encoded_normalized,
        original.trim_end_matches(['/', '\\']).to_owned(),
    ];
    if normalized != from {
        let url_path = format!("/{normalized}");
        prefixes.push(url_path.clone());
        prefixes.push(percent_encode(&url_path));
    }
    if let Some(authority) = normalized
        .strip_prefix("//")
        .filter(|rest| !rest.is_empty())
    {
        prefixes.push(authority.to_owned());
        prefixes.push(percent_encode(authority));
    }
    relocate(
        &mut value,
        &prefixes.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let mut bytes = serde_json::to_vec_pretty(&value)
        .map_err(|e| Error::Metadata(MetadataError::Invalid(e.to_string())))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn relocate_text(text: &str, from: &[&str]) -> String {
    let mut prefixes = from.to_vec();
    prefixes.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let in_file_url = cursor >= "file://".len() && text[..cursor].ends_with("file://");
        let at_boundary = cursor == 0
            || in_file_url
            || text[..cursor]
                .char_indices()
                .next_back()
                .is_some_and(|(index, character)| starts_path(text, index, character));
        if at_boundary
            && let Some(prefix) = prefixes.iter().find(|prefix| {
                text[cursor..].strip_prefix(**prefix).is_some_and(|tail| {
                    tail.is_empty()
                        || tail.starts_with(['/', '\\'])
                        || (in_file_url && tail.starts_with(['#', '?']))
                })
            })
        {
            let tail_start = cursor + prefix.len();
            let tail_end = tail_end(text, tail_start);
            let tail = text[tail_start..tail_end].replace('\\', "/");
            output.push_str(SAMPLE_REPOSITORY);
            output.push_str(&tail);
            cursor = tail_end;
            continue;
        }
        let Some(character) = text[cursor..].chars().next() else {
            break;
        };
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

fn tail_end(text: &str, start: usize) -> usize {
    let mut index = start;
    while let Some(character) = text[index..].chars().next() {
        if ends_path(text, index, character) {
            return index;
        }
        index += character.len_utf8();
    }
    text.len()
}

fn starts_path(text: &str, index: usize, character: char) -> bool {
    ends_path(text, index, character) || character == '='
}

fn ends_path(text: &str, index: usize, character: char) -> bool {
    character.is_whitespace()
        || matches!(character, ';' | ',' | '|')
        || (character == ':' && colon_ends_path(text, index))
}

fn colon_ends_path(text: &str, index: usize) -> bool {
    let rest = &text[index + 1..];
    if rest.starts_with("//") {
        return false;
    }
    rest.starts_with(['/', '\\'])
        || (rest.starts_with(|character: char| character.is_ascii_alphabetic())
            && rest[1..].starts_with(':'))
}

fn relocate(value: &mut serde_json::Value, from: &[&str]) -> Result<(), Error> {
    match value {
        serde_json::Value::String(text) => *text = relocate_text(text, from),
        serde_json::Value::Array(items) => {
            for item in items {
                relocate(item, from)?;
            }
        }
        serde_json::Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut entry) in entries {
                relocate(&mut entry, from)?;
                let key = relocate_text(&key, from);
                if map.insert(key.clone(), entry).is_some() {
                    return Err(Error::Repository(format!(
                        "recorded metadata has more than one object key relocated to {key:?}"
                    )));
                }
            }
        }
        _ => {}
    }
    Ok(())
}
