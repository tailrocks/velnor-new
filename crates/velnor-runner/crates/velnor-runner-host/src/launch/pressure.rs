//! Live host pressure. Configured `max_jobs` is the ceiling, not the live count.
//!
//! Each call grows or shrinks by one. A missing sample holds the previous count.
//! The first advertisement starts at one so a high ceiling cannot stampede.

// Live sampling is macOS-only; the parse helpers below exist on other
// platforms solely for unit tests, so the import follows the same gate.
#[cfg(any(test, target_os = "macos"))]
use std::process::Command;

const GROW_LOAD_PER_CPU_MILLIS: u32 = 750;
const SHRINK_LOAD_PER_CPU_MILLIS: u32 = 1150;
const GIB: u64 = 1024 * 1024 * 1024;
const GROW_MEM: u64 = 8 * GIB;
const SHRINK_MEM: u64 = 4 * GIB;
const GROW_DISK: u64 = 20 * GIB;
const SHRINK_DISK: u64 = 10 * GIB;

/// One host sample. `disk_free == u64::MAX` means disk was not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    /// One-minute load average times 1000.
    pub load_millis: u32,
    /// Online CPUs. Zero is treated as saturated.
    pub ncpu: u32,
    /// Bytes `memory_pressure` still calls free.
    pub mem_available: u64,
    /// Free bytes on the sampled filesystem.
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
    decide(1, 0, ceiling, sample())
}

/// Next poll header. A failed sample does not change `current`.
#[must_use]
pub(crate) fn adjust(current: u32, running: u32, ceiling: u32) -> u32 {
    decide(current, running, ceiling, sample())
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

/// `{ 33.41 42.75 40.01 }` -> `33410`. Later averages are ignored.
#[cfg(any(test, target_os = "macos"))]
#[must_use]
pub(crate) fn parse_loadavg(text: &str) -> Option<u32> {
    text.split_whitespace().find_map(load_token_millis)
}

#[cfg(any(test, target_os = "macos"))]
fn load_token_millis(token: &str) -> Option<u32> {
    let token = token.trim_matches(|c: char| c == '{' || c == '}' || c == ',');
    let (whole, frac) = token.split_once('.').unwrap_or((token, ""));
    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole = whole.parse::<u32>().ok()?;
    let mut millis = 0u32;
    let mut scale = 100u32;
    for digit in frac.chars().take(3) {
        let value = digit.to_digit(10)?;
        millis = millis.saturating_add(value * scale);
        scale /= 10;
    }
    whole.checked_mul(1000)?.checked_add(millis)
}

/// Reads `System-wide memory free percentage: N%`.
#[cfg(any(test, target_os = "macos"))]
#[must_use]
pub(crate) fn parse_memory_pressure_percent(text: &str) -> Option<u8> {
    let rest = text.split("System-wide memory free percentage:").nth(1)?;
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let percent = digits.parse::<u16>().ok()?;
    u8::try_from(percent).ok().filter(|value| *value <= 100)
}

/// `memsize * percent / 100`, saturating at `u64::MAX`.
#[cfg(any(test, target_os = "macos"))]
#[must_use]
pub(crate) fn available_bytes(memsize: u64, percent: u8) -> u64 {
    let product = u128::from(memsize) * u128::from(percent) / 100;
    u64::try_from(product).unwrap_or(u64::MAX)
}

/// Online CPU count. Zero and garbage are `None`.
#[cfg(any(test, target_os = "macos"))]
#[must_use]
pub(crate) fn parse_ncpu(text: &str) -> Option<u32> {
    let count = text.trim().parse::<u32>().ok()?;
    (count > 0).then_some(count)
}

/// Available column of one `df -kP` line, converted from KiB to bytes.
#[cfg(any(test, target_os = "macos"))]
#[must_use]
pub(crate) fn parse_df_avail_kib(line: &str) -> Option<u64> {
    let mut fields = line.split_whitespace();
    let _filesystem = fields.next()?;
    let _blocks = fields.next()?;
    let _used = fields.next()?;
    let avail = fields.next()?.parse::<u64>().ok()?;
    avail.checked_mul(1024)
}

fn sample() -> Option<Sample> {
    #[cfg(target_os = "macos")]
    {
        sample_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn sample_macos() -> Option<Sample> {
    let load_millis = parse_loadavg(&command("sysctl", &["-n", "vm.loadavg"])?)?;
    let ncpu = parse_ncpu(&command("sysctl", &["-n", "hw.ncpu"])?)?;
    let memsize = command("sysctl", &["-n", "hw.memsize"])?
        .trim()
        .parse::<u64>()
        .ok()?;
    let percent = parse_memory_pressure_percent(&command("memory_pressure", &["-Q"])?)?;
    let disk_free = command("df", &["-kP", "/"])
        .and_then(|text| text.lines().rev().find_map(parse_df_avail_kib))
        .unwrap_or(u64::MAX);
    Some(Sample {
        load_millis,
        ncpu,
        mem_available: available_bytes(memsize, percent),
        disk_free,
    })
}

#[cfg(target_os = "macos")]
fn command(bin: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(bin).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        GROW_DISK, GROW_MEM, SHRINK_DISK, SHRINK_MEM, Sample, available_bytes, decide,
        parse_df_avail_kib, parse_loadavg, parse_memory_pressure_percent, parse_ncpu,
    };

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
    fn idle_host_grows_one_slot() {
        let grown = step(500, 64 * GROW_MEM / 8, 100 * GROW_DISK / 20, 1, 1, 8);
        assert_eq!(grown, 2);
    }

    #[test]
    fn second_idle_step_grows_again() {
        let grown = step(500, 64 * GROW_MEM / 8, GROW_DISK, 2, 1, 8);
        assert_eq!(grown, 3);
    }

    #[test]
    fn saturated_host_shrinks_toward_running() {
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
    fn parsers_read_host_text() {
        assert_eq!(parse_loadavg("{ 33.41 42.75 40.01 }"), Some(33_410));
        assert_eq!(parse_loadavg("{ 0.50 0.40 0.30 }"), Some(500));
        assert_eq!(parse_loadavg("nope"), None);
        let pressure = "System-wide memory free percentage: 77%\n";
        assert_eq!(parse_memory_pressure_percent(pressure), Some(77));
        assert_eq!(parse_memory_pressure_percent("free"), None);
        assert_eq!(available_bytes(137_438_953_472, 77), 105_827_994_173);
        assert_eq!(parse_ncpu("18"), Some(18));
        assert_eq!(parse_ncpu("0"), None);
        assert_eq!(parse_ncpu("nope"), None);
        let line = "/dev/disk3s5 1000 100 610000000 84% /System/Volumes/Data";
        assert_eq!(parse_df_avail_kib(line), Some(610_000_000 * 1024));
        assert_eq!(
            parse_df_avail_kib("Filesystem 1024-blocks Used Available"),
            None
        );
    }
}
