use std::collections::HashMap;

pub(super) fn contains_open_authority(
    source: &str,
    aliases: &HashMap<String, Vec<String>>,
) -> bool {
    identifiers(source).iter().any(|identifier| {
        matches!(identifier.as_str(), "open" | "openat" | "openat2")
            || aliases.get(identifier).is_some_and(|path| {
                super::is_path_open_call(path) || super::is_dangerous_file_api(path)
            })
    })
}

fn identifiers(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut identifiers = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = literal_end(bytes, index) {
            index = end;
        } else if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            identifiers.push(source[start..index].to_owned());
        } else {
            index += 1;
        }
    }
    identifiers
}

fn literal_end(bytes: &[u8], index: usize) -> Option<usize> {
    let mut prefix = index;
    if bytes.get(prefix) == Some(&b'b') {
        prefix += 1;
    }
    if bytes.get(prefix) == Some(&b'r') {
        let hash_start = prefix + 1;
        let mut quote = hash_start;
        while bytes.get(quote) == Some(&b'#') {
            quote += 1;
        }
        if bytes.get(quote) != Some(&b'"') {
            return None;
        }
        let hashes = quote - hash_start;
        let mut cursor = quote + 1;
        while cursor < bytes.len() {
            if bytes[cursor] == b'"'
                && bytes.get(cursor + 1..cursor + 1 + hashes) == Some(&vec![b'#'; hashes][..])
            {
                return Some(cursor + 1 + hashes);
            }
            cursor += 1;
        }
        return Some(bytes.len());
    }
    let quote = match bytes.get(prefix) {
        Some(b'"') => b'"',
        _ => return None,
    };
    let mut cursor = prefix + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor = cursor.saturating_add(2);
            continue;
        } else if bytes[cursor] == quote {
            return Some(cursor + 1);
        }
        cursor += 1;
    }
    Some(bytes.len())
}
