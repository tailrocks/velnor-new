// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipFailure {
    IdMismatch,

    MissingResponse,

    ForeignObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedIds {
    pub container_id: String,

    pub volume: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupProof {
    pub container_id: String,

    pub volume: String,
}

impl CleanupProof {
    #[must_use]
    pub fn covers(&self, owned: &OwnedIds) -> bool {
        !self.container_id.is_empty()
            && self.container_id == owned.container_id
            && self.volume == owned.volume
    }
}
