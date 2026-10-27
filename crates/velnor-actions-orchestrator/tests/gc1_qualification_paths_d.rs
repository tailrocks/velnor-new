// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// C22: bounded foreign in-flight writes are charged against the same ceiling,
/// and unknown inventory or unknown foreign exposure blocks publication while
/// ordinary checks stay usable.
pub(crate) fn check_unknown_pressure_blocks() -> Result<(), Box<dyn Error>> {
    let bytes = paths_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let inventory_sum = OPERATING_CEILING_BYTES - bound;
    let generation = paths_generation(
        b"gc1-qualification-paths-pressure",
        b"gc1-qualification-paths-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 222, 1, 2221, "writer")?;
    let charged = decide_admission(&paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        1,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if charged.decision != AdmissionDecision::Blocked {
        return Err(invalid("foreign pressure escaped the ceiling charge"));
    }
    let mut unknown_inventory = paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    );
    unknown_inventory.inventory_bytes = unknown_bytes("gc1-qualification-paths-inventory")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_inventory =
        decide_admission(&unknown_inventory).map_err(|error| invalid(&error.to_string()))?;
    if blocked_inventory.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown mixed inventory admitted"));
    }
    if !blocked_inventory
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(invalid("unknown mixed inventory missed its reason"));
    }
    let mut unknown_foreign = paths_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    );
    unknown_foreign.foreign_inflight_bytes = unknown_bytes("gc1-qualification-paths-foreign")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_foreign =
        decide_admission(&unknown_foreign).map_err(|error| invalid(&error.to_string()))?;
    if blocked_foreign.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown foreign exposure admitted"));
    }
    if !blocked_foreign
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(invalid("unknown foreign exposure missed its reason"));
    }
    Ok(())
}
