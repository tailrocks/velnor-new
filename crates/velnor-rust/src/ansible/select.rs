// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{EventKind, RepoFacts, RepoPath, RepoView};

use super::{playbooks, slug_collision};
use crate::error::DetectError;

pub fn ansible_runs(
    view: &RepoView,
    facts: &RepoFacts,
    _event: EventKind,
    _changed: Option<&BTreeSet<RepoPath>>,
    _unknown: &BTreeSet<RepoPath>,
) -> Result<Option<bool>, DetectError> {
    if playbooks(view)?.is_none() {
        return Ok(None);
    }
    slug_collision(facts)?;
    Ok(Some(true))
}
