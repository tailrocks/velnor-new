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
    Budget, MAIN_KEEP, MAIN_MAX_BORN_SECONDS, MG_MAX_SECONDS, PR_KEEP, PR_MAX_SECONDS,
    PRUNE_FLOOR_SECONDS, prune_plan,
};
pub use run::Run;
pub use scope::{GenScope, Generation};
