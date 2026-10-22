use crate::journal::Journal;

use super::PrSourceTransitionJournal;

#[path = "journal_api/nonce.rs"]
mod nonce;
#[path = "journal_api/reference.rs"]
mod reference;
#[path = "journal_api/retarget.rs"]
mod retarget;

impl Journal {
    /// Open the explicit component API for durable PR source transitions.
    #[must_use]
    pub fn pr_source_transition(&self) -> PrSourceTransitionJournal<'_> {
        PrSourceTransitionJournal { journal: self }
    }
}
