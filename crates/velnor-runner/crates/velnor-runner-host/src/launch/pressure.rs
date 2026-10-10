//! Guest pressure policy. Configured `max_jobs` is a ceiling, not a live count.
//!
//! Each fresh sample grows or shrinks by one. Missing guest metrics hold the
//! previous count; job starts remain separately gated until a fresh sample exists.
//! A new session advertises one slot before the first successful sample.

const GROW_LOAD_PER_CPU_MILLIS: u32 = 750;
const SHRINK_LOAD_PER_CPU_MILLIS: u32 = 1150;
const GIB: u64 = 1024 * 1024 * 1024;
const GROW_MEM: u64 = 8 * GIB;
const SHRINK_MEM: u64 = 4 * GIB;
const GROW_DISK: u64 = 20 * GIB;
const SHRINK_DISK: u64 = 10 * GIB;

/// One selected-guest sample. `disk_free == u64::MAX` means disk was not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    /// One-minute guest load average times 1000.
    pub load_millis: u32,
    /// Guest vCPUs. Zero is treated as saturated.
    pub ncpu: u32,
    /// Guest `MemAvailable` bytes.
    pub mem_available: u64,
    /// Free bytes on the filesystem containing Docker's root.
    pub disk_free: u64,
}

/// Ceiling floored at one. There is no fixed upper clamp.
#[must_use]
pub(crate) const fn clamp_ceiling(ceiling: u32) -> u32 {
    if ceiling == 0 { 1 } else { ceiling }
}

/// Running jobs, but never zero and never above the ceiling.
#[must_use]
pub(crate) const fn floor_of(running: u32, ceiling: u32) -> u32 {
    let ceiling = clamp_ceiling(ceiling);
    let running = if running > ceiling { ceiling } else { running };
    if running == 0 { 1 } else { running }
}

/// Apply one pressure step. `None` holds `current` inside the legal range.
#[must_use]
pub(crate) fn decide(current: u32, running: u32, ceiling: u32, sample: Option<Sample>) -> u32 {
    let ceiling = clamp_ceiling(ceiling);
    let floor = floor_of(running, ceiling);
    match sample {
        Some(sample) => step(current, floor, ceiling, sample),
        None => clamp_range(current, floor, ceiling),
    }
}

/// First `X-ScaleSetMaxCapacity` for this session.
#[must_use]
pub(crate) fn advertise(ceiling: u32) -> u32 {
    floor_of(1, ceiling)
}

/// Later poll header. The trusted guest probe is not wired yet, so this holds
/// the current count and never falls back to host metrics.
#[must_use]
pub(crate) fn adjust(current: u32, running: u32, ceiling: u32) -> u32 {
    decide(current, running, ceiling, None)
}

fn step(current: u32, floor: u32, ceiling: u32, sample: Sample) -> u32 {
    let current = clamp_range(current, floor, ceiling);
    if saturated(sample) {
        return current.saturating_sub(1).max(floor);
    }
    if room(sample) && current < ceiling {
        return current + 1;
    }
    current
}

fn clamp_range(current: u32, floor: u32, ceiling: u32) -> u32 {
    current.clamp(floor, ceiling)
}

const fn saturated(sample: Sample) -> bool {
    load_per_cpu(sample) > SHRINK_LOAD_PER_CPU_MILLIS
        || sample.mem_available < SHRINK_MEM
        || disk_low(sample)
}

const fn room(sample: Sample) -> bool {
    load_per_cpu(sample) <= GROW_LOAD_PER_CPU_MILLIS
        && sample.mem_available >= GROW_MEM
        && disk_ok(sample)
}

const fn disk_low(sample: Sample) -> bool {
    sample.disk_free != u64::MAX && sample.disk_free < SHRINK_DISK
}

const fn disk_ok(sample: Sample) -> bool {
    sample.disk_free == u64::MAX || sample.disk_free >= GROW_DISK
}

const fn load_per_cpu(sample: Sample) -> u32 {
    match sample.load_millis.checked_div(sample.ncpu) {
        Some(value) => value,
        None => u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::{GROW_DISK, GROW_MEM, SHRINK_DISK, SHRINK_MEM, Sample, adjust, advertise, decide};

    fn sample(load_millis: u32, mem: u64, disk: u64) -> Sample {
        Sample {
            load_millis,
            ncpu: 18,
            mem_available: mem,
            disk_free: disk,
        }
    }

    fn step(
        load_millis: u32,
        mem: u64,
        disk: u64,
        current: u32,
        running: u32,
        ceiling: u32,
    ) -> u32 {
        decide(
            current,
            running,
            ceiling,
            Some(sample(load_millis, mem, disk)),
        )
    }

    #[test]
    fn idle_guest_grows_one_slot() {
        let grown = step(500, 64 * GROW_MEM / 8, 100 * GROW_DISK / 20, 1, 1, 8);
        assert_eq!(grown, 2);
    }

    #[test]
    fn second_idle_step_grows_again() {
        let grown = step(500, 64 * GROW_MEM / 8, GROW_DISK, 2, 1, 8);
        assert_eq!(grown, 3);
    }

    #[test]
    fn saturated_guest_shrinks_toward_running() {
        let shrunk = step(2000 * 18, GROW_MEM, GROW_DISK, 4, 2, 8);
        assert_eq!(shrunk, 3);
    }

    #[test]
    fn saturated_does_not_shrink_below_running() {
        let held = step(2000 * 18, GROW_MEM, GROW_DISK, 2, 2, 8);
        assert_eq!(held, 2);
    }

    #[test]
    fn low_memory_shrinks() {
        let shrunk = step(500, SHRINK_MEM - 1, GROW_DISK, 3, 1, 8);
        assert_eq!(shrunk, 2);
    }

    #[test]
    fn low_disk_shrinks_and_unknown_disk_does_not() {
        let shrunk = step(500, GROW_MEM, SHRINK_DISK - 1, 3, 1, 8);
        assert_eq!(shrunk, 2);
        let held = step(900 * 18, GROW_MEM, u64::MAX, 3, 1, 8);
        assert_eq!(held, 3);
    }

    #[test]
    fn mid_load_holds() {
        let held = step(900 * 18, GROW_MEM, GROW_DISK, 3, 1, 8);
        assert_eq!(held, 3);
    }

    #[test]
    fn ceiling_blocks_growth() {
        let held = step(500, GROW_MEM, GROW_DISK, 2, 1, 2);
        assert_eq!(held, 2);
    }

    #[test]
    fn saturated_open_stays_at_one() {
        let opened = step(2000 * 18, GROW_MEM, GROW_DISK, 1, 0, 8);
        assert_eq!(opened, 1);
    }

    #[test]
    fn missing_sample_holds_inside_range() {
        assert_eq!(decide(4, 2, 8, None), 4);
        assert_eq!(decide(0, 0, 8, None), 1);
        assert_eq!(decide(9, 0, 99, None), 9);
    }

    #[test]
    fn new_session_advertises_one_without_guest_metrics() {
        assert_eq!(advertise(0), 1);
        assert_eq!(advertise(8), 1);
    }

    #[test]
    fn production_adjustment_holds_without_guest_metrics() {
        assert_eq!(adjust(1, 0, 8), 1);
        assert_eq!(adjust(3, 1, 8), 3);
        assert_eq!(adjust(1, 2, 8), 2);
    }
}
