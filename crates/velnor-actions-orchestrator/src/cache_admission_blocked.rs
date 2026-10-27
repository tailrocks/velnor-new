// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    AdmissionRequest, BLOCKED_GENERATION_SLOTS_EXHAUSTED, BLOCKED_INSUFFICIENT_HEADROOM,
    BLOCKED_INSUFFICIENT_LOCAL_DISK, BLOCKED_UNKNOWN_FOREIGN_EXPOSURE,
    BLOCKED_UNKNOWN_INVENTORY, BLOCKED_UNKNOWN_LOCAL_DISK, BLOCKED_UNKNOWN_PENDING_WRITES,
    BLOCKED_UNKNOWN_RUNTIME_ENVELOPE, EnvelopeParts, MAX_GENERATION_SLOTS, OuterEnvelope,
};

pub(crate) fn measured_inner_bytes(envelope: &OuterEnvelope) -> Option<u64> {
    match envelope {
        OuterEnvelope::Measured(measured) => Some(measured.inner.archive_bytes),
        OuterEnvelope::Unknown(_) => None,
    }
}

pub(crate) fn inner_identity_binds(request: &AdmissionRequest, envelope: &OuterEnvelope) -> bool {
    match (
        request.candidate_inner_bytes.value,
        measured_inner_bytes(envelope),
    ) {
        (Some(candidate), Some(measured)) => candidate == measured,
        _ => true,
    }
}

pub(crate) fn collect_blocked(
    request: &AdmissionRequest,
    parts: &EnvelopeParts,
    envelope: &OuterEnvelope,
) -> Vec<String> {
    let mut blocked: Vec<String> = Vec::new();
    if request.inventory_bytes.value.is_none() {
        blocked.push(BLOCKED_UNKNOWN_INVENTORY.to_owned());
    }
    if request.pending_owned_bytes.value.is_none() {
        blocked.push(BLOCKED_UNKNOWN_PENDING_WRITES.to_owned());
    }
    if request.foreign_inflight_bytes.value.is_none() {
        blocked.push(BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned());
    }
    if parts.candidate_upper_bound_bytes.value.is_none() || !parts.qualified {
        blocked.push(BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned());
    }
    let bound_binds = inner_identity_binds(request, envelope);
    if !bound_binds {
        blocked.push(BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned());
    }
    if request.occupied_generation_slots >= MAX_GENERATION_SLOTS {
        blocked.push(BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned());
    }
    let headroom_bound = if bound_binds {
        parts.candidate_upper_bound_bytes.value
    } else {
        None
    };
    collect_headroom_block(request, headroom_bound, &mut blocked);
    collect_disk_block(request, &mut blocked);
    blocked.sort();
    blocked.dedup();
    blocked
}

pub(crate) fn collect_headroom_block(
    request: &AdmissionRequest,
    bound: Option<u64>,
    blocked: &mut Vec<String>,
) {
    if let (Some(inventory), Some(pending), Some(foreign), Some(bound)) = (
        request.inventory_bytes.value,
        request.pending_owned_bytes.value,
        request.foreign_inflight_bytes.value,
        bound,
    ) {
        let fits = inventory
            .checked_add(pending)
            .and_then(|total| total.checked_add(foreign))
            .and_then(|total| total.checked_add(bound))
            .is_some_and(|total| total <= request.operating_ceiling_bytes);
        if !fits {
            blocked.push(BLOCKED_INSUFFICIENT_HEADROOM.to_owned());
        }
    }
}

pub(crate) fn collect_disk_block(request: &AdmissionRequest, blocked: &mut Vec<String>) {
    if let (Some(peak), Some(available)) = (
        request.peak_local_disk_bound.value,
        request.available_local_disk.value,
    ) {
        if peak
            .checked_add(request.local_disk_reserve)
            .is_none_or(|required| required > available)
        {
            blocked.push(BLOCKED_INSUFFICIENT_LOCAL_DISK.to_owned());
        }
    } else {
        blocked.push(BLOCKED_UNKNOWN_LOCAL_DISK.to_owned());
    }
}

