// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::WorkflowConfig;

impl WorkflowConfig {
    /// Fixed GitHub repository allowance in bytes.
    ///
    /// Part of the single GitHub-only cache policy configuration. Consumer
    /// configuration cannot request a larger accounting ceiling.
    pub const GITHUB_CACHE_ACCOUNTING_CEILING_BYTES: u64 = 10_000_000_000;

    /// Provisional operating ceiling in bytes.
    ///
    /// Part of the single GitHub-only cache policy configuration. Only a
    /// lower operating ceiling may be used; the accounting ceiling above is
    /// never raised and paid overflow is never enabled.
    pub const GITHUB_CACHE_DEFAULT_OPERATING_CEILING_BYTES: u64 = 8_000_000_000;

    /// Minimum headroom below the accounting ceiling in bytes.
    ///
    /// Part of the single GitHub-only cache policy configuration. Repair
    /// revisions advance only through a reviewed repair record, never through
    /// consumer configuration, and legacy caching has no configuration knob.
    pub const GITHUB_CACHE_MINIMUM_RESERVE_BYTES: u64 = 2_000_000_000;
}

const _: () = assert!(
    WorkflowConfig::GITHUB_CACHE_DEFAULT_OPERATING_CEILING_BYTES
        + WorkflowConfig::GITHUB_CACHE_MINIMUM_RESERVE_BYTES
        == WorkflowConfig::GITHUB_CACHE_ACCOUNTING_CEILING_BYTES,
    "operating ceiling plus minimum reserve must equal the accounting ceiling"
);
