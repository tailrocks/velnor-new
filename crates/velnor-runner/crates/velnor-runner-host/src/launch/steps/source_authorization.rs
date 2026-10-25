use super::source;
use crate::scale_set::EnsureError;

pub(super) fn source_blocked(block: source::SourceBlock) -> EnsureError {
    eprintln!("source_admission=blocked reason={}", block.code());
    EnsureError::SourceBlocked {
        reason: block.code(),
    }
}
