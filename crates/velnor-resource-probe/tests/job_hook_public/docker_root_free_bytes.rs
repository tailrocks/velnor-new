// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_resource_probe::ProbeError;
use velnor_resource_probe::capacity::{StatvfsCounts, record_free_bytes};

const GIB: u64 = 1024 * 1024 * 1024;

#[test]
fn record_free_bytes_is_what_unprivileged_users_can_claim() -> Result<(), ProbeError> {
    let counts = StatvfsCounts {
        blocks: 100 * GIB / 4096,
        bfree: 12 * GIB / 4096,
        bavail: 7 * GIB / 4096,
        frsize: 4096,
    };
    assert_eq!(record_free_bytes(&counts)?, 7 * GIB);
    Ok(())
}
