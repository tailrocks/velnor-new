//! Check total guest capacity for startup and resource admission.

pub(crate) fn checked_guest_capacity(
    worker_cpu_millicores: u64,
    worker_memory_bytes: u64,
    operational_cpu_millicores: u64,
    operational_memory_bytes: u64,
) -> Option<(u64, u64)> {
    Some((
        worker_cpu_millicores.checked_add(operational_cpu_millicores)?,
        worker_memory_bytes.checked_add(operational_memory_bytes)?,
    ))
}

pub(super) fn guest_capacity_fits(
    observed_cpu_millicores: u64,
    observed_memory_bytes: u64,
    required_cpu_millicores: u64,
    required_memory_bytes: u64,
) -> bool {
    observed_cpu_millicores >= required_cpu_millicores
        && observed_memory_bytes >= required_memory_bytes
}

pub(super) fn required_available_memory(
    held_memory_bytes: u64,
    request_memory_bytes: u64,
    retained_memory_bytes: u64,
    operational_reserve_bytes: u64,
) -> Option<u64> {
    held_memory_bytes
        .checked_add(request_memory_bytes)?
        .checked_add(retained_memory_bytes)?
        .checked_add(operational_reserve_bytes)
}

#[cfg(test)]
mod tests {
    use super::{checked_guest_capacity, guest_capacity_fits, required_available_memory};

    #[test]
    fn shared_capacity_accepts_exact_boundary_and_rejects_shortfalls() {
        let (cpu, memory) =
            checked_guest_capacity(8_000, 24, 1_000, 8).expect("The capacity calculation failed.");
        assert!(guest_capacity_fits(cpu, memory, 9_000, 32));
        assert!(!guest_capacity_fits(8_999, memory, 9_000, 32));
        assert!(!guest_capacity_fits(cpu, 31, 9_000, 32));
    }

    #[test]
    fn shared_capacity_reserve_overflow_fails_closed() {
        assert_eq!(checked_guest_capacity(u64::MAX, 1, 1, 0), None);
        assert_eq!(checked_guest_capacity(1, u64::MAX, 0, 1), None);
    }

    #[test]
    fn available_memory_includes_held_new_and_reserve_once() {
        assert_eq!(required_available_memory(5, 10, 3, 2), Some(20));
        assert_eq!(required_available_memory(u64::MAX, 1, 0, 0), None);
    }
}
