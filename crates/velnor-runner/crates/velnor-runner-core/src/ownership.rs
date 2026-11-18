// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const CONTAINER_ID_LEN: usize = 64;

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
        full_container_id(&self.container_id)
            && !self.volume.is_empty()
            && self.container_id == owned.container_id
            && self.volume == owned.volume
    }
}

fn full_container_id(id: &str) -> bool {
    id.len() == CONTAINER_ID_LEN && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}
