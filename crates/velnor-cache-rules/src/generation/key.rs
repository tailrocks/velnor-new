// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenerationError {
    BadSha(String),
    BadSlug(String),
    BadCompat(String),
}

impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSha(text) => write!(f, "not a 40-hex commit sha: {text:?}"),
            Self::BadSlug(text) => write!(f, "not a job or root slug: {text:?}"),
            Self::BadCompat(text) => write!(
                f,
                "not a compat digest of exactly 16 lowercase hex digits: {text:?}"
            ),
        }
    }
}

impl std::error::Error for GenerationError {}

fn is_lower_hex(text: &str) -> bool {
    text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha(String);

impl Sha {
    pub fn parse(text: &str) -> Result<Self, GenerationError> {
        if text.len() == 40 && is_lower_hex(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(GenerationError::BadSha(text.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenKey {
    job: String,
    root: String,
    compat: String,
}

const COMPAT_DIGITS: usize = 16;

fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl GenKey {
    pub fn new(job: &str, root: &str, compat: &str) -> Result<Self, GenerationError> {
        for slug in [job, root] {
            if !is_slug(slug) {
                return Err(GenerationError::BadSlug(slug.to_owned()));
            }
        }
        if compat.len() == COMPAT_DIGITS && is_lower_hex(compat) {
            Ok(Self {
                job: job.to_owned(),
                root: root.to_owned(),
                compat: compat.to_owned(),
            })
        } else {
            Err(GenerationError::BadCompat(compat.to_owned()))
        }
    }

    #[must_use]
    pub fn dir_name(&self) -> String {
        format!("{}.{}-{}", self.job, self.root, self.compat)
    }
}
