//! Guest budget for one host. The configured ceiling stays explicit.
//!
//! CPU, memory, and disk each produce a slot count. The live count is the
//! minimum of those counts and the ceiling. A missing disk sample does not
//! shrink the count. Mac host totals are not inputs.

/// vCPU reserved for one job, after one core is left for the daemon.
const CPU_PER_JOB: u32 = 4;
/// Bytes of guest memory reserved for one job.
const MEM_PER_JOB: u64 = 8 * 1024 * 1024 * 1024;
/// Free guest-disk bytes reserved for one job.
const DISK_PER_JOB: u64 = 20 * 1024 * 1024 * 1024;

/// Live slot count. `disk_free == None` means disk was not observed.
///
/// Zero CPU yields one slot. A zero ceiling yields one slot.
#[must_use]
pub fn guest_slots(ncpu: u32, mem_bytes: u64, disk_free: Option<u64>, ceiling: u32) -> u32 {
    let ceiling = if ceiling == 0 { 1 } else { ceiling };
    let cpu = cpu_slots(ncpu);
    let mem = byte_slots(mem_bytes, MEM_PER_JOB);
    let disk = disk_free.map_or(ceiling, |bytes| byte_slots(bytes, DISK_PER_JOB));
    cpu.min(mem).min(disk).min(ceiling).max(1)
}

const fn cpu_slots(ncpu: u32) -> u32 {
    if ncpu <= 1 {
        1
    } else {
        let usable = ncpu - 1;
        let slots = usable / CPU_PER_JOB;
        if slots == 0 { 1 } else { slots }
    }
}

fn byte_slots(bytes: u64, per_job: u64) -> u32 {
    let count = bytes / per_job;
    u32::try_from(count).unwrap_or(u32::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use super::guest_slots;

    #[test]
    fn eighteen_cpu_guest_yields_four_slots_under_a_high_ceiling() {
        let mem = 121 * 1024 * 1024 * 1024;
        assert_eq!(guest_slots(18, mem, None, 8), 4);
        assert_eq!(guest_slots(18, mem, None, 4), 4);
        assert_eq!(guest_slots(18, mem, None, 2), 2);
    }

    #[test]
    fn memory_disk_and_zero_cpu_can_lower_the_count() {
        let mem = 10 * 1024 * 1024 * 1024;
        assert_eq!(guest_slots(18, mem, None, 8), 1);
        let disk = 25 * 1024 * 1024 * 1024;
        let big = 64 * 1024 * 1024 * 1024;
        assert_eq!(guest_slots(18, big, Some(disk), 8), 1);
        assert_eq!(guest_slots(0, big, None, 8), 1);
        assert_eq!(guest_slots(18, big, None, 0), 1);
    }
}
