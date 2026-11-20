// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const GROW_LOAD_PER_CPU_MILLIS: u64 = 750;
const SHRINK_LOAD_PER_CPU_MILLIS: u64 = 1_150;
const GIB: u64 = 1024 * 1024 * 1024;
const GROW_MEMORY_BYTES: u64 = 8 * GIB;
const SHRINK_MEMORY_BYTES: u64 = 4 * GIB;
const GROW_DISK_BYTES: u64 = 20 * GIB;
const SHRINK_DISK_BYTES: u64 = 10 * GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub load_milli: u64,
    pub cpus: u32,
    pub memory_available_bytes: u64,
    pub docker_root_free_bytes: Option<u64>,
}

#[must_use]
pub(crate) const fn advertise(ceiling: u32) -> u32 {
    floor_of(1, ceiling)
}

#[must_use]
pub(crate) const fn clamp_ceiling(ceiling: u32) -> u32 {
    if ceiling == 0 { 1 } else { ceiling }
}

#[must_use]
pub(crate) const fn floor_of(running: u32, ceiling: u32) -> u32 {
    let ceiling = clamp_ceiling(ceiling);
    let running = if running > ceiling { ceiling } else { running };
    if running == 0 { 1 } else { running }
}

#[must_use]
pub(crate) fn decide(current: u32, running: u32, ceiling: u32, sample: Option<Sample>) -> u32 {
    let ceiling = clamp_ceiling(ceiling);
    let floor = floor_of(running, ceiling);
    let current = current.clamp(floor, ceiling);
    let Some(sample) = sample else {
        return current;
    };
    if saturated(sample) {
        current.saturating_sub(1).max(floor)
    } else if has_room(sample) && current < ceiling {
        current + 1
    } else {
        current
    }
}

#[must_use]
pub(crate) fn adjust_with_sample(
    current: u32,
    running: u32,
    ceiling: u32,
    sample: Option<Sample>,
) -> u32 {
    decide(current, running, ceiling, sample)
}

const fn saturated(sample: Sample) -> bool {
    load_per_cpu(sample) > SHRINK_LOAD_PER_CPU_MILLIS
        || sample.memory_available_bytes < SHRINK_MEMORY_BYTES
        || matches!(sample.docker_root_free_bytes, Some(disk_free) if disk_free < SHRINK_DISK_BYTES)
}

const fn has_room(sample: Sample) -> bool {
    load_per_cpu(sample) <= GROW_LOAD_PER_CPU_MILLIS
        && sample.memory_available_bytes >= GROW_MEMORY_BYTES
        && match sample.docker_root_free_bytes {
            Some(disk_free) => disk_free >= GROW_DISK_BYTES,
            None => true,
        }
}

const fn load_per_cpu(sample: Sample) -> u64 {
    match sample.load_milli.checked_div(sample.cpus as u64) {
        Some(value) => value,
        None => u64::MAX,
    }
}
