const GROW_LOAD_PER_CPU_MILLIS: u32 = 750;
const SHRINK_LOAD_PER_CPU_MILLIS: u32 = 1150;
const GIB: u64 = 1024 * 1024 * 1024;
const GROW_MEM: u64 = 8 * GIB;
const SHRINK_MEM: u64 = 4 * GIB;
const GROW_DISK: u64 = 20 * GIB;
const SHRINK_DISK: u64 = 10 * GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub load_millis: u32,

    pub ncpu: u32,

    pub mem_available: u64,

    pub disk_free: u64,
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
    match sample {
        Some(sample) => step(current, floor, ceiling, sample),
        None => clamp_range(current, floor, ceiling),
    }
}

#[must_use]
pub(crate) fn advertise(ceiling: u32) -> u32 {
    floor_of(1, ceiling)
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
