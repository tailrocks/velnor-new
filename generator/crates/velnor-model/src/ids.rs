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
    pub fn is_within(&self, ancestor: &Self) -> bool {
        self == ancestor
            || self
                .0
                .strip_prefix(ancestor.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
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
