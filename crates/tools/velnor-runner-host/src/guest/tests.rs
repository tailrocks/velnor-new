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
