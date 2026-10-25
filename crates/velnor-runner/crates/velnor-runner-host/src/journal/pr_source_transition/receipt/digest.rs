use sha2::{Digest, Sha256};

use crate::journal::pr_source_transition::model::plan::TransitionPlan;
use crate::journal::pr_source_transition::model::validation::hex;

pub(super) fn hash_text(hash: &mut Sha256, value: &str) {
    hash.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
    hash.update(value.as_bytes());
}

pub(super) fn line_count(message: &str, expected: &str) -> u8 {
    match message
        .lines()
        .filter(|line| *line == expected)
        .take(2)
        .count()
    {
        0 => 0,
        1 => 1,
        _ => 2,
    }
}

pub(super) fn message_digest(message: &str) -> String {
    text_digest(b"velnor-pr-commit-message-v1\0", message)
}

pub(crate) fn nonce_message_digest(nonce: &str, plan: &TransitionPlan) -> String {
    let message = format!(
        "Velnor pull request validation\n\nvelnor-pr-validation-nonce:v1:{nonce}\n\n{}",
        plan.commit_identity.sign_off_line()
    );
    message_digest(&message)
}

pub(super) fn text_digest(prefix: &[u8], value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(prefix);
    hash_text(&mut digest, value);
    hex(&digest.finalize())
}
