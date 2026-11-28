// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod key;
mod pick;
mod prune;
mod run;
mod scope;

pub use key::{GenKey, GenerationError, Sha};
pub use pick::{Want, pick};
pub use prune::{
    MAIN_KEEP, MAIN_MAX_BORN_SECONDS, MG_MAX_SECONDS, PR_KEEP, PR_MAX_SECONDS, PRUNE_FLOOR_SECONDS,
    PrunePlan, prune_plan,
};
pub use run::{Head, Run, RunEvent};
pub use scope::{GenScope, Generation};
