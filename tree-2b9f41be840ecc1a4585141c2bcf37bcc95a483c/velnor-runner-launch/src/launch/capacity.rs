//! How many workers one session may start.
//!
//! `VELNOR_MAX_JOBS` is total capacity, not free slots. The poll header uses it.
//! `VELNOR_ADMIT_TARGET`, when higher, is how many starts this session keeps polling for.

use std::cell::Cell;

use super::steps::Idle;

use velnor_runner_host::listen::parse_job_capacity;

const POLL_DEFAULT: usize = 8;
const POLL_MAX: usize = 8;
const POLL_MAX_MULTI: usize = 24;
const POLL_WIDE_DEFAULT: usize = 90;
const POLL_WIDE_MAX: usize = 120;

/// What `poll_and_drive` does with one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// Start one worker. `stop` ends the session after that start.
    Start {
        /// Leave the session after this start.
        stop: bool,
    },
    /// Poll again. No start and no acknowledgement.
    Stay,
    /// End the session. No start and no acknowledgement.
    Stop,
    /// Delete this batch. This does not free a running slot.
    Ack {
        /// Leave the session after the acknowledgement.
        stop: bool,
    },
    /// Running count is at capacity. The launch stays unacquired.
    Hold,
    /// Two ids, or another message that must stay on the queue. No start.
    Error,
}

/// Inputs for [`admit`]. Counts are for this call, not free slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Seat {
    /// Clamped job capacity for this session.
    pub(crate) capacity: u32,
    /// Starts this session should reach. Equals capacity unless admission is widened.
    pub(crate) target: u32,
    /// Workers this call already started. An exited start does not keep the slot.
    pub(crate) started: u32,
    /// Launch rows that still hold a permit. Unresolved rows count. History does not.
    pub(crate) occupied: u32,
    /// Running runner containers.
    pub(crate) running: u32,
    /// `statistics.totalAssignedJobs` for this poll. Zero when the poll has none.
    pub(crate) assigned: u32,
    /// Classified poll.
    pub(crate) idle: Idle,
    /// Start or completion notices only. A full slot can acknowledge them.
    pub(crate) progress: bool,
}

/// Decide one poll. A free slot starts the next job. Historical `started` does not.
///
/// An uncovered assignment is not acknowledged. One full slot holds that offer.
/// It does not hide another free slot.
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
    // Only a running container covers `totalAssignedJobs`. An exited start does not.
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

/// Launch and scale always read Docker. A full `started` count must not hide a free slot.
#[must_use]
pub(crate) const fn needs_running(idle: Idle) -> bool {
    matches!(idle, Idle::Launch | Idle::Scale)
}

/// True when session statistics must not mint another pair.
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

/// Restores the prior thread-local capacity when dropped.
#[must_use]
pub(crate) struct CapacityGuard {
    previous: Option<u32>,
}

impl Drop for CapacityGuard {
    fn drop(&mut self) {
        JOB_CAPACITY_OVERRIDE.with(|slot| slot.set(self.previous));
    }
}

/// Prefer `max_jobs` from the host file over `VELNOR_MAX_JOBS` on this thread.
///
/// Zero becomes 1. The value is not clamped to a fixed maximum.
pub(crate) fn install_job_capacity(max_jobs: u32) -> CapacityGuard {
    let stored = if max_jobs == 0 { 1 } else { max_jobs };
    let previous = JOB_CAPACITY_OVERRIDE.with(|slot| {
        let previous = slot.get();
        slot.set(Some(stored));
        previous
    });
    CapacityGuard { previous }
}

/// Installed host capacity, else `VELNOR_MAX_JOBS`.
///
/// Unset, empty, zero, or unparsable env is 1. There is no fixed upper clamp.
#[must_use]
pub(crate) fn job_capacity() -> u32 {
    if let Some(value) = JOB_CAPACITY_OVERRIDE.with(Cell::get) {
        return value;
    }
    parse_job_capacity(std::env::var("VELNOR_MAX_JOBS").ok().as_deref())
}

/// `VELNOR_ADMIT_TARGET`. Unset, empty, unparsable, or below `capacity` is `capacity`.
#[must_use]
pub(crate) fn admit_target(capacity: u32) -> u32 {
    parse_admit_target(
        capacity,
        std::env::var("VELNOR_ADMIT_TARGET").ok().as_deref(),
    )
}

/// Parser used by [`admit_target`].
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

/// `VELNOR_LAUNCH_POLLS` for `capacity`. Unset or unparsable is 8.
#[must_use]
pub(super) fn poll_bound(capacity: u32) -> usize {
    poll_limit(
        capacity,
        std::env::var("VELNOR_LAUNCH_POLLS").ok().as_deref(),
    )
}

/// `VELNOR_LAUNCH_POLLS` when the admission target exceeds capacity.
/// Unset or unparsable is 90. Clamped to 1..=120.
#[must_use]
pub(super) fn poll_bound_wide() -> usize {
    wide_poll_limit(std::env::var("VELNOR_LAUNCH_POLLS").ok().as_deref())
}

/// Clamp the widened poll bound. Unset or unparsable is 90.
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

/// Clamp a poll bound. Capacity 1 uses 1..=8. Above that, 1..=24.
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
