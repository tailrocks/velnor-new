// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

const RUN_KEY: &str = "r4250-a1";
const GROUP_ID: &str = "rust-f7-job";
const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

mod fixtures_ledger;
mod fixtures_merge;
mod fixtures_platform;
mod lifecycle;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

use lifecycle::{
    check_push_deferring_ledger_blocks, check_push_warm_cache_hit_passes,
    check_unaffected_with_ledger_mismatches,
};
#[test]
fn gc1_t50_lifecycle_ledgers() -> Result<(), Box<dyn Error>> {
    check_push_warm_cache_hit_passes()?;
    check_push_deferring_ledger_blocks()?;
    check_unaffected_with_ledger_mismatches()?;
    Ok(())
}
