// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::cachekey::{
    FormatInputs, cache_format_id, mbx_cache_generation, mbx_qualification_cache_generation,
};
use velnor_actions_contract::workflow::qualification_cache_lineage::validate_qualification_cache_generation;

const MBX_VERSION: &str = "1.91.0";
const LEGACY_GENERATION: &str = "velnor-mbx-1.91.0-share-out-dir-disabled-v1";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn task_format(generation: &str) -> Result<String, Box<dyn Error>> {
    Ok(cache_format_id(&FormatInputs {
        adapter: "mbx".to_owned(),
        format: "task-result".to_owned(),
        generation: generation.to_owned(),
    })?)
}

#[test]
fn production_generation_is_truthful_and_leaves_the_legacy_era() -> Result<(), Box<dyn Error>> {
    let production = mbx_cache_generation(MBX_VERSION);
    if production == LEGACY_GENERATION {
        return Err(fail(
            "production generation aliases the legacy flag-off era",
        ));
    }
    if production.contains("disabled") {
        return Err(fail("production generation still claims a disabled flag"));
    }
    if !production.starts_with("velnor-mbx-") || !production.contains(MBX_VERSION) {
        return Err(fail("production generation lost its owned slot shape"));
    }
    if production == mbx_cache_generation("1.92.0") {
        return Err(fail("production generation ignores the mbx version"));
    }
    if task_format(&production)? == task_format(LEGACY_GENERATION)? {
        return Err(fail("task results from the flag-off era stay reachable"));
    }
    Ok(())
}

#[test]
fn qualification_keeps_its_own_flag_off_slot() -> Result<(), Box<dyn Error>> {
    let qualification = mbx_qualification_cache_generation(MBX_VERSION);
    if qualification != LEGACY_GENERATION {
        return Err(fail("qualification slot moved off its flag-off label"));
    }
    validate_qualification_cache_generation(&qualification, MBX_VERSION)
        .map_err(|error| fail(&format!("qualification slot rejected: {error}")))?;
    if validate_qualification_cache_generation(&mbx_cache_generation(MBX_VERSION), MBX_VERSION)
        .is_ok()
    {
        return Err(fail("qualification accepted the production slot"));
    }
    if validate_qualification_cache_generation(&format!("{LEGACY_GENERATION}-v2"), MBX_VERSION)
        .is_ok()
    {
        return Err(fail("qualification accepted an unowned slot"));
    }
    Ok(())
}
