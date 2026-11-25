// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_model::RustMember;

pub(super) struct Spec<'a> {
    name: Name<'a>,
    version: Option<Partial>,
    address: Option<String>,
}

enum Name<'a> {
    Exact(String),
    Glob(&'a str),
}

struct Partial {
    numbers: Vec<u64>,
    pre: String,
    build: Option<String>,
}

impl Spec<'_> {
    pub(super) fn matches(&self, member: &RustMember, repositories: &[&Path]) -> bool {
        let named = match &self.name {
            Name::Exact(name) => member.name.as_str() == name,
            Name::Glob(pattern) => glob(
                &pattern.chars().collect::<Vec<_>>(),
                &member.name.as_str().chars().collect::<Vec<_>>(),
            ),
        };
        named
            && self
                .address
                .as_deref()
                .is_none_or(|address| located(member, repositories, address))
            && self.version.as_ref().is_none_or(|version| {
                member
                    .version
                    .as_deref()
                    .and_then(parse_version)
                    .is_some_and(|actual| version.matches(&actual))
            })
    }
}

fn located(member: &RustMember, repositories: &[&Path], address: &str) -> bool {
    member.manifest.parent().is_some_and(|directory| {
        repositories.iter().any(|repository| {
            Path::new(&encode(
                &repository.join(directory.as_str()).to_string_lossy(),
            )) == Path::new(address)
        })
    })
}

impl Partial {
    fn matches(&self, actual: &Self) -> bool {
        self.numbers.len() <= actual.numbers.len()
            && self.numbers == actual.numbers[..self.numbers.len()]
            && self.pre == actual.pre
            && self
                .build
                .as_ref()
                .is_none_or(|build| actual.build.as_ref() == Some(build))
    }
}

pub(super) fn parse(spec: &str) -> Option<Spec<'_>> {
    if spec.contains(['*', '?', '[', ']']) {
        return Some(Spec {
            name: Name::Glob(spec),
            version: None,
            address: None,
        });
    }
    let Some((scheme, location)) = spec.split_once("://") else {
        return if spec.contains('#') {
            None
        } else {
            versioned(spec, None)
        };
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme.strip_prefix("path+").unwrap_or(&scheme) != "file" {
        return None;
    }
    let (address, fragment) = location
        .split_once('#')
        .map_or((location, None), |(address, fragment)| {
            (address, Some(fragment))
        });
    match fragment {
        Some(fragment) if !fragment.starts_with(|c: char| c.is_ascii_digit()) => {
            versioned(fragment, Some(address.to_owned()))
        }
        Some(version) => Some(Spec {
            name: Name::Exact(decode(last_segment(address))),
            version: Some(parse_version(version)?),
            address: Some(address.to_owned()),
        }),
        None => Some(Spec {
            name: Name::Exact(decode(last_segment(address))),
            version: None,
            address: Some(address.to_owned()),
        }),
    }
}

fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_control()
            || !byte.is_ascii()
            || matches!(
                byte,
                b' ' | b'"' | b'#' | b'%' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}'
            )
        {
            encoded.push_str(&format!("%{byte:02X}"));
        } else {
            encoded.push(char::from(byte));
        }
    }
    encoded
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let Some(byte) = escaped {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn versioned(spec: &str, address: Option<String>) -> Option<Spec<'_>> {
    match spec.split_once(['@', ':']) {
        Some((name, version)) => Some(Spec {
            name: Name::Exact(name.to_owned()),
            version: Some(parse_version(version)?),
            address,
        }),
        None => Some(Spec {
            name: Name::Exact(spec.to_owned()),
            version: None,
            address,
        }),
    }
}

fn parse_version(text: &str) -> Option<Partial> {
    let (text, build) = text
        .split_once('+')
        .map_or((text, None), |(text, build)| (text, Some(build.to_owned())));
    let (numbers, pre) = text
        .split_once('-')
        .map_or((text, ""), |(numbers, pre)| (numbers, pre));
    let numbers = numbers
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<Vec<u64>>>()?;
    let valid = (1..=3).contains(&numbers.len()) && (pre.is_empty() || numbers.len() == 3);
    valid.then(|| Partial {
        numbers,
        pre: pre.to_owned(),
        build,
    })
}

pub(super) fn named(pattern: &str, name: &str) -> bool {
    if pattern.contains(['*', '?', '[', ']']) {
        glob(
            &pattern.chars().collect::<Vec<_>>(),
            &name.chars().collect::<Vec<_>>(),
        )
    } else {
        pattern == name
    }
}

fn last_segment(address: &str) -> &str {
    address.rsplit('/').next().unwrap_or(address)
}

fn glob(pattern: &[char], name: &[char]) -> bool {
    let (mut at, mut on) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        match pattern.get(at) {
            Some('*') => {
                at += 1;
                star = Some((at, on));
                continue;
            }
            Some(_) if on < name.len() => {
                if let Some(next) = token(pattern, at, name[on]) {
                    at = next;
                    on += 1;
                    continue;
                }
            }
            None if on == name.len() => return true,
            _ => {}
        }
        match star {
            Some((resume, from)) if from < name.len() => {
                star = Some((resume, from + 1));
                at = resume;
                on = from + 1;
            }
            _ => return false,
        }
    }
}

fn token(pattern: &[char], at: usize, symbol: char) -> Option<usize> {
    match pattern[at] {
        '?' => Some(at + 1),
        '[' => {
            let (negated, members, after) = class(&pattern[at + 1..])?;
            (members_contain(members, symbol) != negated).then(|| pattern.len() - after.len())
        }
        literal => (literal == symbol).then_some(at + 1),
    }
}

fn class(pattern: &[char]) -> Option<(bool, &[char], &[char])> {
    let (negated, body) = match pattern.split_first() {
        Some(('!', rest)) => (true, rest),
        _ => (false, pattern),
    };
    let end = body.iter().skip(1).position(|symbol| *symbol == ']')? + 1;
    Some((negated, &body[..end], &body[end + 1..]))
}

fn members_contain(members: &[char], symbol: char) -> bool {
    let mut index = 0;
    while index < members.len() {
        if members.get(index + 1) == Some(&'-') && index + 2 < members.len() {
            if (members[index]..=members[index + 2]).contains(&symbol) {
                return true;
            }
            index += 3;
        } else {
            if members[index] == symbol {
                return true;
            }
            index += 1;
        }
    }
    false
}
