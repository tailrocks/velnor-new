// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::ExitCode;

use velnor_actions_orchestrator::{inventory_digest, parse_inventory_bytes};

use super::single_line;
use crate::dispatch::fail_internal;

pub(super) fn run_cleanup(inventory_bytes: &[u8], candidate_ids: &[u64]) -> ExitCode {
    let inventory = match parse_inventory_bytes(inventory_bytes) {
        Ok(inventory) => inventory,
        Err(error) => return fail_internal(&single_line(&error.to_string())),
    };
    if let Err(error) = inventory_digest(&inventory) {
        return fail_internal(&single_line(&error.to_string()));
    }
    for candidate in candidate_ids {
        let known = inventory
            .entries
            .iter()
            .any(|entry| entry.cache_id == *candidate);
        if !known {
            return fail_internal("cache_cleanup_candidate_unknown");
        }
    }
    ExitCode::SUCCESS
}
