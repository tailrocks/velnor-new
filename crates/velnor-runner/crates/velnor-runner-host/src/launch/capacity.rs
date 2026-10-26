// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use super::steps::Idle;

const POLL_DEFAULT: usize = 8;
const POLL_MAX: usize = 8;
const POLL_MAX_MULTI: usize = 24;
const POLL_WIDE_DEFAULT: usize = 90;
const POLL_WIDE_MAX: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admit {
    Start { stop: bool },

    Stay,

    Stop,

    Ack { stop: bool },

    Hold,

    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Seat {
    pub(crate) capacity: u32,

    pub(crate) target: u32,

    pub(crate) started: u32,

    pub(crate) occupied: u32,

    pub(crate) running: u32,

    pub(crate) assigned: u32,

    pub(crate) idle: Idle,

    pub(crate) progress: bool,
}

#[must_use]
pub(crate) const fn admit(seat: Seat) -> Admit {
    match seat.idle {
        Idle::Blocked => Admit::Error,
        Idle::Empty => {
            if started_done(seat) {
                Admit::Stop
            } else {
                Admit::Stay
            }
        }
        Idle::Ack => Admit::Ack {
            stop: started_done(seat),
        },
        Idle::Launch => admit_launch(seat),
        Idle::Scale => admit_scale(seat),
    }
}

const fn admit_launch(seat: Seat) -> Admit {
    if slot_full(seat) {
        return if started_done(seat) {
            Admit::Stop
        } else {
            Admit::Hold
        };
    }
    Admit::Start {
        stop: at_limit(seat),
    }
}

const fn admit_scale(seat: Seat) -> Admit {
    if !scale_covered(seat) && !slot_full(seat) {
        return Admit::Start {
            stop: at_limit(seat),
        };
    }
    if !scale_covered(seat) {
        if seat.progress {
            return Admit::Ack {
                stop: started_done(seat),
            };
        }
        return Admit::Hold;
    }
    Admit::Ack {
        stop: seat.capacity == 1 || started_done(seat),
    }
}

const fn scale_covered(seat: Seat) -> bool {
    seat.running >= seat.assigned
}

const fn slot_full(seat: Seat) -> bool {
    seat.occupied >= seat.capacity || seat.running >= seat.capacity
}

const fn started_done(seat: Seat) -> bool {
    seat.started >= limit(seat)
}

const fn at_limit(seat: Seat) -> bool {
    seat.started.saturating_add(1) >= limit(seat)
}

const fn limit(seat: Seat) -> u32 {
    if seat.target > seat.capacity {
        seat.target
    } else {
        seat.capacity
    }
}

#[must_use]
pub(crate) const fn needs_running(idle: Idle) -> bool {
    matches!(idle, Idle::Launch | Idle::Scale)
}

#[must_use]
pub(crate) fn statistics_blocked(
    occupied: u32,
    running: u32,
    capacity: u32,
    population: i64,
) -> bool {
    if population <= 0 || occupied >= capacity || running >= capacity {
        return true;
    }
    i64::from(running) >= population
}

thread_local! {
    static JOB_CAPACITY_OVERRIDE: Cell<Option<u32>> = const { Cell::new(None) };
}

#[must_use]
pub(crate) struct CapacityGuard {
    previous: Option<u32>,
}

impl Drop for CapacityGuard {
    fn drop(&mut self) {
        JOB_CAPACITY_OVERRIDE.with(|slot| slot.set(self.previous));
    }
}

pub(crate) fn install_job_capacity(max_jobs: u32) -> CapacityGuard {
    let stored = if max_jobs == 0 { 1 } else { max_jobs };
    let previous = JOB_CAPACITY_OVERRIDE.with(|slot| {
        let previous = slot.get();
        slot.set(Some(stored));
        previous
    });
    CapacityGuard { previous }
}

#[must_use]
pub(crate) fn job_capacity() -> u32 {
    if let Some(value) = JOB_CAPACITY_OVERRIDE.with(Cell::get) {
        return value;
    }
    parse_job_capacity(std::env::var("VELNOR_MAX_JOBS").ok().as_deref())
}

#[must_use]
pub(crate) fn parse_job_capacity(text: Option<&str>) -> u32 {
    let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return 1;
    };
    match text.parse::<u32>() {
        Ok(0) | Err(_) => 1,
        Ok(parsed) => parsed,
    }
}

#[must_use]
pub(crate) fn admit_target(capacity: u32) -> u32 {
    parse_admit_target(
        capacity,
        std::env::var("VELNOR_ADMIT_TARGET").ok().as_deref(),
    )
}

#[must_use]
pub(crate) fn parse_admit_target(capacity: u32, raw: Option<&str>) -> u32 {
    let Some(text) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return capacity;
    };
    let Ok(parsed) = text.parse::<u32>() else {
        return capacity;
    };
    if parsed < capacity { capacity } else { parsed }
}

#[must_use]
pub(super) fn poll_bound(capacity: u32) -> usize {
    poll_limit(
        capacity,
        std::env::var("VELNOR_LAUNCH_POLLS").ok().as_deref(),
    )
}

#[must_use]
pub(super) fn poll_bound_wide() -> usize {
    wide_poll_limit(std::env::var("VELNOR_LAUNCH_POLLS").ok().as_deref())
}

#[must_use]
pub(crate) fn wide_poll_limit(raw: Option<&str>) -> usize {
    let Some(text) = raw else {
        return POLL_WIDE_DEFAULT;
    };
    let Ok(bound) = text.parse::<usize>() else {
        return POLL_WIDE_DEFAULT;
    };
    bound.clamp(1, POLL_WIDE_MAX)
}

#[must_use]
pub(crate) fn poll_limit(capacity: u32, raw: Option<&str>) -> usize {
    let max = if capacity > 1 {
        POLL_MAX_MULTI
    } else {
        POLL_MAX
    };
    let Some(text) = raw else {
        return POLL_DEFAULT;
    };
    let Ok(bound) = text.parse::<usize>() else {
        return POLL_DEFAULT;
    };
    bound.clamp(1, max)
}
