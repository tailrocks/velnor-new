//! How many workers one session may start.
//!
//! `VELNOR_MAX_JOBS` is total capacity, not free slots. The poll header uses it.

use super::steps::Idle;

const JOB_MAX: u32 = 8;
const POLL_DEFAULT: usize = 8;
const POLL_MAX: usize = 8;
const POLL_MAX_MULTI: usize = 24;

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
    /// Workers this call already started, including the statistics runner.
    pub(crate) started: u32,
    /// Running launch containers. Unused when [`needs_running`] is false.
    pub(crate) running: u32,
    /// Classified poll.
    pub(crate) idle: Idle,
}

/// Decide one poll. Capacity 1 leaves after the first start.
#[must_use]
pub(crate) const fn admit(seat: Seat) -> Admit {
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
    if seat.started < seat.capacity && seat.running < seat.capacity {
        return Admit::Start {
            stop: seat.started.saturating_add(1) >= seat.capacity,
        };
    }
    // Capacity 1 leaves. A larger capacity leaves only after enough starts.
    Admit::Ack {
        stop: seat.capacity == 1 || seat.started >= seat.capacity,
    }
}

/// True when the running count can change this poll's decision.
#[must_use]
pub(crate) const fn needs_running(capacity: u32, started: u32, idle: Idle) -> bool {
    match idle {
        Idle::Launch | Idle::Scale => started < capacity,
        Idle::Empty | Idle::Ack | Idle::Blocked => false,
    }
}

/// `VELNOR_MAX_JOBS`. Unset, empty, zero, or unparsable is 1. Clamped to 1..=8.
#[must_use]
pub(crate) fn job_capacity() -> u32 {
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

/// `VELNOR_LAUNCH_POLLS` for `capacity`. Unset or unparsable is 8.
#[must_use]
pub(super) fn poll_bound(capacity: u32) -> usize {
    poll_limit(
        capacity,
        std::env::var("VELNOR_LAUNCH_POLLS").ok().as_deref(),
    )
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
