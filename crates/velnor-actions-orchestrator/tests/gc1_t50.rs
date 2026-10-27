// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! F7 READER-FIRST lifecycle repair: stage scheduling plus ledger winner semantics.
//!
//! Readers run on every event (affected-only) so a push writer never depends on
//! a skipped job under GitHub `success()` scheduling. Writers keep the trusted
//! push gate and always run when affected; the `needs_prepare` coverage signal
//! flows as ledger data, never as a job-level gate. Warm writers emit
//! cache-hit ledgers instead of preparation.

#[expect(
    dead_code,
    unused_imports,
    clippy::duplicate_mod,
    reason = "stage include covers gate helpers; only the reader-first subset is used"
)]
#[path = "../src/crate_jobs_stage.rs"]
mod crate_jobs_stage;

use std::error::Error;

const RUN_KEY: &str = "r4250-a1";
const GROUP_ID: &str = "rust-f7-job";
const GROUP_READER_ID: &str = "rust-f7-job-read";
const GROUP_WRITER_ID: &str = "rust-f7-job-write";
const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

mod fixtures_ledger;
mod fixtures_merge;
mod fixtures_platform;
mod lifecycle;
mod scheduling;

use lifecycle::{
    check_pr_forged_writer_stays_corrupt, check_push_absent_writer_with_reader_complete_passes,
    check_push_cold_writer_wins, check_push_defers_without_writer_blocks,
    check_push_warm_cache_hit_passes, check_unaffected_with_ledger_mismatches,
};
use scheduling::{
    check_push_skipped_reader_rejected, check_reader_runs_where_writer_runs,
    check_signal_flows_as_data_not_gate,
};

#[test]
fn gc1_t50_reader_first_scheduling() -> Result<(), Box<dyn Error>> {
    check_reader_runs_where_writer_runs()?;
    check_push_skipped_reader_rejected()?;
    check_signal_flows_as_data_not_gate()?;
    Ok(())
}

#[test]
fn gc1_t50_lifecycle_ledgers() -> Result<(), Box<dyn Error>> {
    check_push_warm_cache_hit_passes()?;
    check_push_cold_writer_wins()?;
    check_push_absent_writer_with_reader_complete_passes()?;
    check_push_defers_without_writer_blocks()?;
    check_pr_forged_writer_stays_corrupt()?;
    check_unaffected_with_ledger_mismatches()?;
    Ok(())
}
