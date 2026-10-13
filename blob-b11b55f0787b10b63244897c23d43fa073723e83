//! Small shared SQL guards for the journal subtree.

use crate::error::HostError;

pub(super) fn one_row(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) fn token_rejected(token: &str) -> bool {
    token.is_empty() || token.chars().any(|ch| matches!(ch, '\'' | '"'))
}
