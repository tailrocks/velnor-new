// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RustMember;

pub(super) struct Spec<'a> {
    name: Name<'a>,
    version: Option<Partial>,
}

enum Name<'a> {
    Exact(&'a str),
    Glob(&'a str),
}

struct Partial {
    numbers: Vec<u64>,
    pre: String,
    build: Option<String>,
}

impl Spec<'_> {
    pub(super) fn matches(&self, member: &RustMember) -> bool {
        let named = match self.name {
            Name::Exact(name) => member.name.as_str() == name,
            Name::Glob(pattern) => glob(pattern.as_bytes(), member.name.as_str().as_bytes()),
        };
        named
            && self.version.as_ref().is_none_or(|version| {
                member
                    .version
                    .as_deref()
                    .and_then(parse_version)
                    .is_some_and(|actual| version.matches(&actual))
            })
    }
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
        });
    }
    let Some((scheme, location)) = spec.split_once("://") else {
        return if spec.contains('#') {
            None
        } else {
            versioned(spec)
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
            versioned(fragment)
        }
        Some(version) => Some(Spec {
            name: Name::Exact(last_segment(address)),
            version: Some(parse_version(version)?),
        }),
        None => Some(Spec {
            name: Name::Exact(last_segment(address)),
            version: None,
        }),
    }
}

fn versioned(spec: &str) -> Option<Spec<'_>> {
    match spec.split_once(['@', ':']) {
        Some((name, version)) => Some(Spec {
            name: Name::Exact(name),
            version: Some(parse_version(version)?),
        }),
        None => Some(Spec {
            name: Name::Exact(spec),
            version: None,
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

fn last_segment(address: &str) -> &str {
    address.rsplit('/').next().unwrap_or(address)
}

fn glob(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|skip| glob(rest, &name[skip..])),
        Some((b'?', rest)) => name.split_first().is_some_and(|(_, tail)| glob(rest, tail)),
        Some((b'[', rest)) => match (class(rest), name.split_first()) {
            (Some((negated, members, after)), Some((&byte, tail))) => {
                members_contain(members, byte) != negated && glob(after, tail)
            }
            _ => false,
        },
        Some((&byte, rest)) => name
            .split_first()
            .is_some_and(|(&head, tail)| head == byte && glob(rest, tail)),
    }
}

fn class(pattern: &[u8]) -> Option<(bool, &[u8], &[u8])> {
    let (negated, body) = match pattern.split_first() {
        Some((b'!', rest)) => (true, rest),
        _ => (false, pattern),
    };
    let end = body.iter().skip(1).position(|byte| *byte == b']')? + 1;
    Some((negated, &body[..end], &body[end + 1..]))
}

fn members_contain(members: &[u8], byte: u8) -> bool {
    let mut index = 0;
    while index < members.len() {
        if members.get(index + 1) == Some(&b'-') && index + 2 < members.len() {
            if (members[index]..=members[index + 2]).contains(&byte) {
                return true;
            }
            index += 3;
        } else {
            if members[index] == byte {
                return true;
            }
            index += 1;
        }
    }
    false
}
