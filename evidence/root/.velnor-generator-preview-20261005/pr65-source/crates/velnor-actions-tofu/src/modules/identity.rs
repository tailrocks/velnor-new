//! M2 module identity digests (recorded, no reuse yet).

use velnor_actions_contract::{canonical_json_bytes, digest_b3};

/// M2 digest over module identity records.
///
/// Each record is `(file, name, source, target, content)`; the digest
/// binds module source spellings to resolved targets and content.
#[must_use]
pub fn identities_digest(records: &[(&str, &str, &str, &str, &str)]) -> String {
    let mut sorted = records.to_vec();
    sorted.sort_unstable();
    canonical_json_bytes(&sorted)
        .map_or_else(|_| digest_b3(b"modules_error"), |bytes| digest_b3(&bytes))
}
