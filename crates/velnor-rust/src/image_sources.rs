// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BaseScope, CacheKey, Host, RepoFacts, Step, StepKind};

use crate::base_cache::sources_locks;
use crate::plan::step;
use crate::units::units;

#[must_use]
pub fn image_sources_restore(facts: &RepoFacts) -> Vec<Step> {
    let Some(unit) = units(facts).into_iter().next() else {
        return Vec::new();
    };
    vec![
        sources_locks(facts),
        step(StepKind::CacheRestore(CacheKey::Sources {
            host: Host,
            lane: unit.cache_root(facts),
            scope: BaseScope::Shared,
        })),
    ]
}
