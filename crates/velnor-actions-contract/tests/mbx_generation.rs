// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::cachekey::mbx_qualification_cache_generation;
use velnor_actions_contract::workflow::qualification_cache_lineage::validate_qualification_cache_generation;

const MBX_VERSION: &str = "1.91.0";
const LEGACY_GENERATION: &str = "velnor-mbx-1.91.0-share-out-dir-disabled-v1";
const FOREIGN_FLAGS_ENABLED_GENERATION: &str =
    "velnor-mbx-1.91.0-share-out-dir-build-scripts-enabled-v1";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

#[test]
fn qualification_keeps_its_own_flag_off_slot() -> Result<(), Box<dyn Error>> {
    let qualification = mbx_qualification_cache_generation(MBX_VERSION);
    if qualification != LEGACY_GENERATION {
        return Err(fail("qualification slot moved off its flag-off label"));
    }
    validate_qualification_cache_generation(&qualification, MBX_VERSION)
        .map_err(|error| fail(&format!("qualification slot rejected: {error}")))?;
    if validate_qualification_cache_generation(FOREIGN_FLAGS_ENABLED_GENERATION, MBX_VERSION)
        .is_ok()
    {
        return Err(fail("qualification accepted a flags-enabled slot"));
    }
    if validate_qualification_cache_generation(&format!("{LEGACY_GENERATION}-v2"), MBX_VERSION)
        .is_ok()
    {
        return Err(fail("qualification accepted an unowned slot"));
    }
    Ok(())
}
