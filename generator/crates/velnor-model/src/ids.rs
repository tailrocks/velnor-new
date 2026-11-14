// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidId {
    pub kind: &'static str,
    pub value: String,
}

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {:?}", self.kind, self.value)
    }
}

impl std::error::Error for InvalidId {}

macro_rules! id {
    ($name:ident, $valid:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Cow<'static, str>);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
                let value = value.into();
                if $valid(value.as_bytes()) {
                    Ok(Self(Cow::Owned(value)))
                } else {
                    Err(InvalidId {
                        kind: stringify!($name),
                        value,
                    })
                }
            }

            #[must_use]
            pub const fn from_static(value: &'static str) -> Self {
                assert!($valid(value.as_bytes()), "invalid static id");
                Self(Cow::Borrowed(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id!(RepoPath, repo_path);
id!(PathGlob, repo_path);
id!(PackageName, text);
id!(TargetName, text);
id!(RunnerLabel, text);
id!(JobId, identifier);
id!(HelperVersion, version);
id!(DownloadUrl, download_url);
id!(Sha256, sha256);

impl RepoPath {
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| Self(Cow::Owned(parent.to_owned())))
    }

    #[must_use]
    pub fn sibling(&self, name: &Self) -> Self {
        match self.parent() {
            Some(parent) => Self(Cow::Owned(format!("{parent}/{name}"))),
            None => name.clone(),
        }
    }
}

impl PathGlob {
    #[must_use]
    pub fn matches(&self, path: &RepoPath) -> bool {
        let pattern: Vec<&str> = self.as_str().split('/').collect();
        let segments: Vec<&str> = path.as_str().split('/').collect();
        reachable(&pattern, &segments)[1..]
            .iter()
            .any(|reached| *reached)
    }
}

fn reachable(pattern: &[&str], path: &[&str]) -> Vec<bool> {
    let mut row = vec![false; path.len() + 1];
    row[0] = true;
    for segment in pattern {
        let mut next = vec![false; path.len() + 1];
        if *segment == "**" {
            let mut any = false;
            for (reached, slot) in row.iter().zip(next.iter_mut()) {
                any |= *reached;
                *slot = any;
            }
        } else {
            for (index, text) in path.iter().enumerate() {
                next[index + 1] = row[index] && wildcard(segment, text);
            }
        }
        row = next;
    }
    row
}

fn wildcard(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let Some(last) = parts.pop() else {
        return rest.is_empty();
    };
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

impl HelperVersion {
    #[must_use]
    pub fn series(&self) -> Option<(u64, u64)> {
        let mut parts = self.0.split('.');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }
}

const fn text(value: &[u8]) -> bool {
    let mut index = 0;
    while index < value.len() {
        if value[index] < 0x20 || value[index] == 0x7f {
            return false;
        }
        index += 1;
    }
    !value.is_empty()
}

const fn identifier(value: &[u8]) -> bool {
    if value.is_empty() || value[0].is_ascii_digit() || value[0] == b'-' {
        return false;
    }
    let mut index = 0;
    while index < value.len() {
        let byte = value[index];
        if !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
            return false;
        }
        index += 1;
    }
    true
}

const fn repo_path(value: &[u8]) -> bool {
    if !text(value) {
        return false;
    }
    let mut start = 0;
    let mut index = 0;
    while index <= value.len() {
        if index == value.len() || value[index] == b'/' {
            let length = index - start;
            if length == 0
                || (length == 1 && value[start] == b'.')
                || (length == 2 && value[start] == b'.' && value[start + 1] == b'.')
            {
                return false;
            }
            start = index + 1;
        } else if value[index] == b'\\' {
            return false;
        }
        index += 1;
    }
    true
}

const fn version(value: &[u8]) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut index = 0;
    while index < value.len() {
        let byte = value[index];
        if !(byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'+') {
            return false;
        }
        index += 1;
    }
    true
}

const fn download_url(value: &[u8]) -> bool {
    let prefix = b"https://";
    if value.len() <= prefix.len() {
        return false;
    }
    if !value[prefix.len()].is_ascii_alphanumeric() {
        return false;
    }
    let mut index = 0;
    while index < value.len() {
        let byte = value[index];
        if index < prefix.len() && byte != prefix[index] {
            return false;
        }
        if !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'.' | b'_' | b'~' | b':' | b'/' | b'%' | b'+' | b'@' | b'=' | b'&' | b'?' | b'-'
            ))
        {
            return false;
        }
        index += 1;
    }
    true
}

const fn sha256(value: &[u8]) -> bool {
    if value.len() != 64 {
        return false;
    }
    let mut index = 0;
    while index < value.len() {
        let byte = value[index];
        if !(byte.is_ascii_digit() || (byte >= b'a' && byte <= b'f')) {
            return false;
        }
        index += 1;
    }
    true
}
