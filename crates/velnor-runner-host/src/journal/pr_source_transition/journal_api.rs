// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Journal;

use super::PrSourceTransitionJournal;

#[path = "journal_api/nonce.rs"]
mod nonce;
#[path = "journal_api/reference.rs"]
mod reference;
#[path = "journal_api/retarget.rs"]
mod retarget;

impl Journal {
    #[must_use]
    pub fn pr_source_transition(&self) -> PrSourceTransitionJournal<'_> {
        PrSourceTransitionJournal { journal: self }
    }
}
