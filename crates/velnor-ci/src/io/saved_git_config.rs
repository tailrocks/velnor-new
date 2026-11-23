// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const ALLOWED: &[(&str, &[&str])] = &[
    (
        "core",
        &[
            "repositoryformatversion",
            "filemode",
            "bare",
            "logallrefupdates",
            "ignorecase",
            "precomposeunicode",
            "symlinks",
        ],
    ),
    ("extensions", &["objectformat"]),
    ("remote", &["url", "fetch", "tagopt"]),
];
const REMOTE_SCHEMES: &[&str] = &["https://", "http://", "ssh://"];
const TOKEN_PREFIXES: &[&str] = &[
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "gldt-",
    "glrt-",
    "glptt-",
    "glcbt-",
    "glft-",
    "gloas-",
    "glagent-",
    "glsoat-",
];
const TOKEN_HEX_LEN: usize = 40;
const TOKEN_BODY_LEN: usize = 20;
const MAX_UNESCAPE_PASSES: usize = 4;
const MAX_BASIC_PAYLOAD: usize = 512;
const AUTHORIZATION: &str = "authorization";
const BASIC: &str = "basic";

pub(super) fn is_plain(text: &str) -> bool {
    if has_token_prefix(&text.to_ascii_lowercase()) {
        return false;
    }
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.contains(['#', ';', '\\', '\r', '\0']) {
            return false;
        }
        let entry = if line.starts_with('[') {
            let Some((name, rest)) = header(line) else {
                return false;
            };
            section = name;
            rest
        } else {
            line
        };
        if !entry.is_empty() && !plain_entry(&section, entry) {
            return false;
        }
    }
    true
}

fn header(line: &str) -> Option<(String, &str)> {
    let close = line.find(']')?;
    let inside = &line[1..close];
    let name_end = inside
        .find(|each: char| !(each.is_ascii_alphanumeric() || each == '-'))
        .unwrap_or(inside.len());
    let (name, tail) = inside.split_at(name_end);
    let tail = tail.trim_start();
    let named = tail.is_empty()
        || tail.starts_with('.')
        || (tail.len() >= 2 && tail.starts_with('"') && tail.ends_with('"'));
    (!name.is_empty() && named && !tail.contains('\\'))
        .then(|| (name.to_ascii_lowercase(), line[close + 1..].trim()))
}

fn plain_entry(section: &str, entry: &str) -> bool {
    if entry.contains('"') {
        return false;
    }
    let (key, value) = match entry.split_once('=') {
        Some((key, value)) => (key.trim(), Some(value.trim())),
        None => (entry.trim(), None),
    };
    let key = key.to_ascii_lowercase();
    let Some((_, keys)) = ALLOWED.iter().find(|(name, _)| *name == section) else {
        return false;
    };
    keys.contains(&key.as_str()) && (key != "url" || value.is_some_and(plain_remote))
}

pub(super) fn names_credential(text: &str) -> bool {
    variants(text).is_none_or(|variants| variants.iter().any(|text| names_in(text)))
}

fn names_in(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    has_token_prefix(&lower)
        || names_basic_authorization(text)
        || lower
            .split(|each: char| each.is_whitespace() || matches!(each, '\'' | '"' | '<' | '>'))
            .flat_map(embedded_urls)
            .any(|url| !plain_remote(url))
}

fn variants(text: &str) -> Option<Vec<String>> {
    let mut seen = vec![text.to_owned()];
    let mut frontier = vec![text.to_owned()];
    for _ in 0..=MAX_UNESCAPE_PASSES {
        let mut next = Vec::new();
        for current in &frontier {
            let joined = current.replace(['"', '\''], "");
            let derived = [false, true]
                .map(|form| unescape_once(current, form))
                .into_iter()
                .chain(Some(joined).filter(|joined| joined != current));
            for decoded in derived {
                if !seen.contains(&decoded) {
                    seen.push(decoded.clone());
                    next.push(decoded);
                }
            }
        }
        if next.is_empty() {
            return Some(seen);
        }
        frontier = next;
    }
    None
}

fn names_basic_authorization(text: &str) -> bool {
    text.to_ascii_lowercase()
        .match_indices(AUTHORIZATION)
        .any(|(at, _)| {
            let rest = text[at + AUTHORIZATION.len()..].trim_start_matches(|each: char| {
                each.is_whitespace() || matches!(each, ':' | '=' | '\'' | '"')
            });
            rest.get(..BASIC.len())
                .is_some_and(|word| word.eq_ignore_ascii_case(BASIC))
                && basic_payload_credential(
                    rest[BASIC.len()..]
                        .trim_start_matches(|each: char| each.is_whitespace() || each == '\''),
                )
        })
}

fn basic_payload_credential(payload: &str) -> bool {
    let mut bits = 0_u32;
    let mut filled = 0_u32;
    for (seen, each) in payload.bytes().enumerate() {
        let value = match each {
            b'A'..=b'Z' => each - b'A',
            b'a'..=b'z' => each - b'a' + 26,
            b'0'..=b'9' => each - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return false,
        };
        if seen == MAX_BASIC_PAYLOAD {
            return true;
        }
        bits = (bits << 6) | u32::from(value);
        filled += 6;
        if filled >= 8 {
            filled -= 8;
            if bits >> filled == u32::from(b':') {
                return true;
            }
            bits &= (1 << filled) - 1;
        }
    }
    false
}

fn scheme_start(word: &str, at: usize) -> usize {
    word[..at]
        .char_indices()
        .rev()
        .find(|(_, each)| !(each.is_ascii_alphanumeric() || matches!(each, '+' | '.' | '-')))
        .map_or(0, |(index, each)| index + each.len_utf8())
}

fn embedded_urls(word: &str) -> impl Iterator<Item = &str> {
    let mut from = 0;
    std::iter::from_fn(move || {
        let at = from + word.get(from..)?.find("://")?;
        from = at + "://".len();
        let start = scheme_start(word, at);
        let end = word[from..]
            .find("://")
            .map_or(word.len(), |next| scheme_start(word, from + next));
        let url = word[start..end]
            .trim_start_matches(|each: char| !each.is_ascii_alphabetic())
            .trim_end_matches([')', ']', '}', ',', ';', '.', ':']);
        Some(url)
    })
}

fn has_token_prefix(text: &str) -> bool {
    TOKEN_PREFIXES.iter().any(|prefix| {
        text.match_indices(prefix).any(|(at, _)| {
            text[at + prefix.len()..]
                .bytes()
                .take_while(|each| each.is_ascii_alphanumeric() || matches!(each, b'_' | b'-'))
                .take(TOKEN_BODY_LEN)
                .count()
                >= TOKEN_BODY_LEN
        })
    })
}

fn looks_like_token(text: &str) -> bool {
    has_token_prefix(text)
        || text
            .split(|each: char| !each.is_ascii_hexdigit())
            .any(|run| run.len() >= TOKEN_HEX_LEN)
}

fn unescaped(segment: &str) -> Option<String> {
    let mut current = segment.to_owned();
    for _ in 0..=MAX_UNESCAPE_PASSES {
        let decoded = unescape_once(&current, false);
        if decoded == current {
            return Some(current);
        }
        current = decoded;
    }
    None
}

fn unescape_once(text: &str, form: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = bytes
            .get(at + 1..at + 3)
            .filter(|_| bytes[at] == b'%')
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let Some(byte) = escaped {
            out.push(byte);
            at += 3;
        } else {
            out.push(if form && bytes[at] == b'+' {
                b' '
            } else {
                bytes[at]
            });
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn plain_remote(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    REMOTE_SCHEMES.iter().any(|scheme| {
        url.strip_prefix(scheme).is_some_and(|rest| {
            let mut segments = rest.split('/');
            let authority = segments.next().unwrap_or("");
            !authority.is_empty()
                && !url.contains('?')
                && !segments
                    .any(|segment| unescaped(segment).is_none_or(|text| looks_like_token(&text)))
                && match authority.rsplit_once('@') {
                    None => true,
                    Some((userinfo, _)) => {
                        *scheme == "ssh://"
                            && !userinfo.contains([':', '%', '@'])
                            && !looks_like_token(userinfo)
                    }
                }
        })
    })
}
