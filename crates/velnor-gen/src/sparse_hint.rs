// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{CheckoutScope, RepoPath, RepoView};
use velnor_rust::{bytes_outside_lanes, detect};

use crate::{GenerateError, SPARSE_HINT_HELPER};

pub const SPARSE_SUGGESTION_BYTES: u64 = 100 * 1024 * 1024;

pub fn sparse_note(
    view: &RepoView,
    sizes: &BTreeMap<RepoPath, u64>,
) -> Result<Option<String>, GenerateError> {
    let facts = detect(view)?;
    if facts.config.checkout == CheckoutScope::Sparse {
        return Ok(None);
    }
    Ok(bytes_outside_lanes(&facts, sizes)
        .filter(|bytes| *bytes > SPARSE_SUGGESTION_BYTES)
        .map(|bytes| {
            format!(
                "{} MiB of tracked files lie outside the paths Rust jobs read; set sparse_checkout = true in .velnor/rust-ci.toml so each job checks out only what it reads (needs velnor-ci {}.{}.{} or newer to name the [[inputs]] line a test that reads an unlisted file needs; see CONFIG.md)",
                bytes / (1024 * 1024),
                SPARSE_HINT_HELPER.0,
                SPARSE_HINT_HELPER.1,
                SPARSE_HINT_HELPER.2
            )
        }))
}
