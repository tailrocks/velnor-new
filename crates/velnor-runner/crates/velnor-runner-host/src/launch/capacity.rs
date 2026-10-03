//! How many workers one session may start.
//!
//! `VELNOR_MAX_JOBS` is total capacity, not free slots. The poll header uses it.
//! `VELNOR_ADMIT_TARGET`, when higher, is how many starts this session keeps polling for.

use std::cell::Cell;

use super::steps::Idle;

const JOB_MAX: u32 = 8;
const POLL_DEFAULT: usize = 8;
const POLL_MAX: usize = 8;
const POLL_MAX_MULTI: usize = 24;
const POLL_WIDE_DEFAULT: usize = 90;
const POLL_WIDE_MAX: usize = 120;

/// What `poll_and_drive` does with one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admit {
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
    /// Workers this call already started, including the statistics runner.
    pub(crate) started: u32,
    /// Running launch containers. Unused when [`needs_running`] is false.
    pub(crate) running: u32,
    /// `statistics.totalAssignedJobs` for this poll. Zero when the poll has none.
    pub(crate) assigned: u32,
    /// Classified poll.
    pub(crate) idle: Idle,
}

/// Decide one poll. Capacity 1 leaves after the first start.
///
/// A target equal to capacity keeps the historic decision. A higher target
/// keeps polling until that many workers have started, and holds a launch
/// while `running` is already at capacity.
#[must_use]
pub(crate) const fn admit(seat: Seat) -> Admit {
    if seat.target > seat.capacity {
        admit_above(seat)
    } else {
        admit_exact(seat)
    }
}

/// Historic decision. Compares counts with capacity, not a wider target.
const fn admit_exact(seat: Seat) -> Admit {
    match seat.idle {
        Idle::Blocked => Admit::Error,
        Idle::Empty => admit_empty(seat),
        Idle::Ack => Admit::Ack {
            stop: seat.started >= seat.capacity,
        },
        Idle::Launch => admit_launch(seat),
        Idle::Scale => admit_scale(seat),
    }
}

/// Target is above capacity. Do not stop just because `started` reached capacity.
const fn admit_above(seat: Seat) -> Admit {
    match seat.idle {
        Idle::Blocked => Admit::Error,
        Idle::Empty => above_empty(seat),
        Idle::Ack => Admit::Ack {
            stop: seat.started >= seat.target,
        },
        Idle::Launch => above_launch(seat),
        Idle::Scale => above_scale(seat),
    }
}

const fn above_empty(seat: Seat) -> Admit {
    if seat.started >= seat.target {
        Admit::Stop
    } else {
        Admit::Stay
    }
}

const fn above_launch(seat: Seat) -> Admit {
    if seat.started >= seat.target {
        return Admit::Stop;
    }
    if seat.running >= seat.capacity {
        return Admit::Hold;
    }
    Admit::Start {
        stop: seat.started.saturating_add(1) >= seat.target,
    }
}

const fn above_scale(seat: Seat) -> Admit {
    if seat.started >= seat.target {
        return Admit::Ack { stop: true };
    }
    if scale_covered(seat) || seat.running >= seat.capacity {
        return Admit::Ack { stop: false };
    }
    Admit::Start {
        stop: seat.started.saturating_add(1) >= seat.target,
    }
}

const fn scale_covered(seat: Seat) -> bool {
    // An exited start still increments `started`. Only a running container
    // covers `totalAssignedJobs`. Otherwise the next queued job is acknowledged
    // and never minted while this session stays under capacity.
    seat.running >= seat.assigned
}

const fn admit_empty(seat: Seat) -> Admit {
    if seat.started >= seat.capacity {
        Admit::Stop
    } else {
        Admit::Stay
    }
}

const fn admit_launch(seat: Seat) -> Admit {
    if seat.running >= seat.capacity && seat.started < seat.capacity {
        return Admit::Hold;
    }
    if seat.started >= seat.capacity {
        return Admit::Stop;
    }
    Admit::Start {
        stop: seat.started.saturating_add(1) >= seat.capacity,
    }
}

const fn admit_scale(seat: Seat) -> Admit {
    if !scale_covered(seat) && seat.started < seat.capacity && seat.running < seat.capacity {
        return Admit::Start {
            stop: seat.started.saturating_add(1) >= seat.capacity,
        };
    }
    // Capacity 1 leaves. A larger capacity leaves only after enough starts.
    // A population already covered by this session is acknowledged, not minted again.
    Admit::Ack {
        stop: seat.capacity == 1 || seat.started >= seat.capacity,
    }
}

/// True when the running count can change this poll's decision.
///
/// Launch and scale ask while `started` is below the admission target.
/// A target equal to capacity matches `started < capacity`.
#[must_use]
pub(crate) const fn needs_running(capacity: u32, target: u32, started: u32, idle: Idle) -> bool {
    let limit = if target > capacity { target } else { capacity };
    match idle {
        Idle::Launch | Idle::Scale => started < limit,
        Idle::Empty | Idle::Ack | Idle::Blocked => false,
    }
}

thread_local! {
    static JOB_CAPACITY_OVERRIDE: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Clears the thread-local capacity when dropped.
#[must_use]
pub(crate) struct CapacityGuard;

impl Drop for CapacityGuard {
    fn drop(&mut self) {
        JOB_CAPACITY_OVERRIDE.with(|slot| slot.set(None));
    }
}

/// Prefer `max_jobs` from the host file over `VELNOR_MAX_JOBS` on this thread.
pub(crate) fn install_job_capacity(max_jobs: u32) -> CapacityGuard {
    let clamped = if max_jobs == 0 {
        1
    } else {
        max_jobs.min(JOB_MAX)
    };
    JOB_CAPACITY_OVERRIDE.with(|slot| slot.set(Some(clamped)));
    CapacityGuard
}

/// Installed host capacity, else `VELNOR_MAX_JOBS`.
///
/// Unset, empty, zero, or unparsable env is 1. Clamped to 1..=8.
#[must_use]
pub(crate) fn job_capacity() -> u32 {
    if let Some(value) = JOB_CAPACITY_OVERRIDE.with(Cell::get) {
        return value;
    }
    parse_job_capacity(std::env::var("VELNOR_MAX_JOBS").ok().as_deref())
}

/// Parser used by [`job_capacity`].
#[must_use]
pub(crate) fn parse_job_capacity(text: Option<&str>) -> u32 {
    let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return 1;
    };
    text.parse::<u32>()
        .map_or(1, |parsed| parsed.clamp(1, JOB_MAX))
}

/// `VELNOR_ADMIT_TARGET`. Unset, empty, unparsable, or below `capacity` is `capacity`.
/// Clamped to `capacity`..=8.
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
    if parsed < capacity {
        return capacity;
    }
    let hi = JOB_MAX.max(capacity);
    if parsed > hi { hi } else { parsed }
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
