// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    prefix: String,
    listed: Vec<String>,
}

impl Scope {
    #[must_use]
    pub fn new(prefix: &str, listed: Vec<String>) -> Self {
        Self {
            prefix: prefix.trim_matches('/').to_owned(),
            listed,
        }
    }

    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        within(path, &self.prefix) || self.listed.iter().any(|listed| within(path, listed))
    }
}

fn within(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}
