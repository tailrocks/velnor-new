// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope(pub String);

impl Scope {
    #[must_use]
    pub fn branch(name: &str) -> Self {
        Self(format!("refs/heads/{name}"))
    }

    #[must_use]
    pub fn tag(name: &str) -> Self {
        Self(format!("refs/tags/{name}"))
    }

    #[must_use]
    pub fn pull_merge(number: u64) -> Self {
        Self(format!("refs/pull/{number}/merge"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub scope: Scope,
    pub key: String,
    pub version: String,
    pub size_bytes: u64,
    pub created: Timestamp,
    pub last_access: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId {
    pub scope: Scope,
    pub key: String,
    pub version: String,
}

impl Entry {
    #[must_use]
    pub fn id(&self) -> EntryId {
        EntryId {
            scope: self.scope.clone(),
            key: self.key.clone(),
            version: self.version.clone(),
        }
    }
}
