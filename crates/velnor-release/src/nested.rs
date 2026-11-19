// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::plan::ReleasePlan;

impl ReleasePlan {
    pub fn refuse_roots_below_git_root(&self) -> Result<(), ReleaseError> {
        match self.roots.iter().find(|root| root.root != ".") {
            Some(root) => Err(ReleaseError::NestedReleasePlzRoot {
                root: root.root.clone(),
            }),
            None => Ok(()),
        }
    }
}
