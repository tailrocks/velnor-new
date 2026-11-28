// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod access;
mod entry;
mod evict;
mod generation;
mod lookup;
mod reserve;

pub use access::{Access, Event, access_for};
pub use entry::{Entry, EntryId, Scope, Timestamp};
pub use evict::{
    DEFAULT_CACHE_BUDGET_BYTES, ENTRY_LIFETIME_SECONDS, REPOSITORY_LIMIT_BYTES, evict, is_expired,
};
pub use generation::{
    GenKey, GenScope, Generation, GenerationError, Head, MAIN_KEEP, MAIN_MAX_BORN_SECONDS,
    MG_MAX_SECONDS, PR_KEEP, PR_MAX_SECONDS, PRUNE_FLOOR_SECONDS, PrunePlan, Run, RunEvent, Sha,
    Want, pick, prune_plan,
};
pub use lookup::lookup;
pub use reserve::{AlreadyExists, reserve};
