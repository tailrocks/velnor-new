// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Slots;
use crate::rust_runner::ReservationId;

fn slots_quarantined(numbers: &[u64]) -> Slots {
    let mut slots = Slots::new(8);
    for number in numbers {
        if let Some(reservation) = ReservationId::new(*number) {
            slots.quarantine_at_startup(reservation);
        }
    }
    slots
}

fn lowest(numbers: &[u64]) -> Option<u64> {
    slots_quarantined(numbers)
        .next_free(1)
        .map(ReservationId::get)
}

#[test]
fn the_lowest_id_that_is_neither_used_nor_quarantined_is_allocated() {
    assert_eq!(lowest(&[]), Some(1));
    assert_eq!(lowest(&[1, 2, 4]), Some(3));
    assert_eq!(lowest(&[2, 3]), Some(1));
    assert_eq!(lowest(&[99]), Some(1));
}

#[test]
fn a_leftover_numbered_u64_max_never_stalls_allocation() {
    assert_eq!(lowest(&[u64::MAX]), Some(1));
    assert_eq!(lowest(&[1, u64::MAX]), Some(2));
}

#[test]
fn only_quarantined_slots_do_not_count_as_work() {
    assert!(slots_quarantined(&[]).only_quarantined());
    assert!(slots_quarantined(&[7, 9]).only_quarantined());
}

#[test]
fn allocation_moves_on_from_the_last_id_and_wraps_at_the_end() {
    let slots = slots_quarantined(&[3, u64::MAX]);
    let from = |start: u64| slots.next_free(start).map(ReservationId::get);
    assert_eq!(from(2), Some(2));
    assert_eq!(from(3), Some(4));
    assert_eq!(from(u64::MAX), Some(1));
    assert_eq!(from(0), Some(1));
}
