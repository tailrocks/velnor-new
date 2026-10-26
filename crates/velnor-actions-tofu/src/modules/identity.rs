// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{canonical_json_bytes, digest_b3};

#[must_use]
pub fn identities_digest(records: &[(&str, &str, &str, &str, &str)]) -> String {
    let mut sorted = records.to_vec();
    sorted.sort_unstable();
    canonical_json_bytes(&sorted)
        .map_or_else(|_| digest_b3(b"modules_error"), |bytes| digest_b3(&bytes))
}
